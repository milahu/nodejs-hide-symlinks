extern crate libc;

#[macro_use]
extern crate redhook;

use std::collections::HashMap;
use std::ffi::CString;
use std::env;

unsafe fn is_symlink(statxbuf: *const libc::statx) -> bool {
    // https://man7.org/linux/man-pages/man7/inode.7.html
    const S_IFMT:   u16 = 0o170000; // bit mask for the file type bit field
    const S_IFLNK:  u16 = 0o120000; // symbolic link
    return ((*statxbuf).stx_mode & S_IFMT) == S_IFLNK;
}

unsafe fn str_of_chars(cstr: *const libc::c_char) -> &'static str {
    return std::str::from_utf8(std::ffi::CStr::from_ptr(cstr).to_bytes()).unwrap();
}

unsafe fn str_of_chars_len(cstr: *const libc::c_char, clen: libc::ssize_t) -> &'static str {
    if clen < 0 {
        return "";
    }
    return std::str::from_utf8(std::slice::from_raw_parts(cstr as *const u8, clen as usize)).unwrap();
}

unsafe fn string_of_statxbuf(statxbuf: *const libc::statx) -> String {
    return format!("{{ stx_ino: {}, stx_mode: {:#o}, is_symlink: {:?} }}", (*statxbuf).stx_ino, (*statxbuf).stx_mode, is_symlink(statxbuf));
}

hook! {
    // https://docs.rs/libc/latest/libc/fn.syscall.html
    unsafe fn syscall(
        num: libc::c_long,
        a1: *mut libc::c_void,
        a2: *mut libc::c_void,
        a3: *mut libc::c_void,
        a4: *mut libc::c_void,
        a5: *mut libc::c_void
    ) -> libc::c_long => my_syscall {

        if num == libc::SYS_statx {
            println!("hooking numeric syscall statx");
            let dirfd = a1 as libc::c_int;
            let path = a2 as *const libc::c_char;
            let flags = a3 as libc::c_int;
            let mask = a4 as libc::c_uint;
            let statxbuf = a5 as *mut libc::statx;
            return statx_hooked(dirfd, path, flags, mask, statxbuf) as libc::c_long;
        }

        if num == libc::SYS_readlink {
            println!("hooking numeric syscall readlink");
            let path = a1 as *const libc::c_char;
            let buf = a2 as *mut libc::c_char;
            let bufsz = a3 as libc::size_t;
            return readlink_hooked(path, buf, bufsz) as libc::c_long;
        }

        if num == libc::SYS_readlinkat {
            println!("hooking numeric syscall readlinkat");
            let dirfd = a1 as libc::c_int;
            let pathname = a2 as *const libc::c_char;
            let buf = a3 as *mut libc::c_char;
            let bufsz = a4 as libc::size_t;
            return readlinkat_hooked(dirfd, pathname, buf, bufsz) as libc::c_long;
        }

        if num == libc::SYS_open {
            println!("hooking numeric syscall open");
            let path = a1 as *const libc::c_char;
            let oflag = a2 as libc::c_int;
            return open_hooked(path, oflag) as libc::c_long;
        }

        if num == libc::SYS_openat {
            println!("hooking numeric syscall openat");
            let dirfd = a1 as libc::c_int;
            let pathname = a2 as *const libc::c_char;
            let flags = a3 as libc::c_int;
            return openat_hooked(dirfd, pathname, flags) as libc::c_long;
        }

        println!("keeping numeric syscall {}", num);
        return real!(syscall)(num, a1, a2, a3, a4, a5);
    }
}

hook! {
    unsafe fn statx(
        dirfd: libc::c_int,
        path: *const libc::c_char,
        flags: libc::c_int,
        mask: libc::c_uint,
        statxbuf: *mut libc::statx // https://docs.rs/libc/0.2.103/libc/struct.statx.html
    ) -> libc::c_int => statx_hooked {
        let mut retval = real!(statx)(dirfd, path, flags, mask, statxbuf);
        eprintln!("nodejs-hide-symlinks statx({}, \"{}\", {}, {}, &statxbuf) -> retval: {:?}, statxbuf: {}", dirfd, str_of_chars(path), flags, mask, retval, string_of_statxbuf(statxbuf));
        return retval;
    }
}

hook! {
    // https://docs.rs/libc/latest/libc/fn.readlink.html
    unsafe fn readlink(
        path: *const libc::c_char,
        buf: *mut libc::c_char,
        bufsz: libc::size_t
    ) -> libc::ssize_t => readlink_hooked {
        let mut retval = real!(readlink)(path, buf, bufsz);
        println!("hooking syscall readlink(\"{}\", &buf, bufsz) -> retval: {:?}, buf: {}", str_of_chars(path), retval, str_of_chars_len(buf, retval));
        return retval;
    }
}

hook! {
    // https://docs.rs/libc/latest/libc/fn.readlinkat.html
    unsafe fn readlinkat(
        dirfd: libc::c_int,
        pathname: *const libc::c_char,
        buf: *mut libc::c_char,
        bufsz: libc::size_t
    ) -> libc::ssize_t => readlinkat_hooked {
        let mut retval = real!(readlinkat)(dirfd, pathname, buf, bufsz);
        println!("hooking syscall readlinkat({}, \"{}\", &buf, bufsz) -> retval: {:?}, buf: {}", dirfd, str_of_chars(pathname), retval, str_of_chars_len(buf, retval));
        return retval;
    }
}

hook! {
    // https://docs.rs/libc/latest/libc/fn.open.html
    unsafe fn open(
        path: *const libc::c_char,
        oflag: libc::c_int
    ) -> libc::c_int => open_hooked {
        let mut retval = real!(open)(path, oflag);
        println!("hooking syscall open(\"{}\", {}) -> retval: {:?}", str_of_chars(path), oflag, retval);
        return retval;
    }
}

hook! {
    // https://docs.rs/libc/latest/libc/fn.openat.html
    unsafe fn openat(
        dirfd: libc::c_int,
        pathname: *const libc::c_char,
        flags: libc::c_int
    ) -> libc::c_int => openat_hooked {
        let mut retval = real!(openat)(dirfd, pathname, flags);
        println!("hooking syscall openat({}, \"{}\", {}) -> retval: {:?}", dirfd, str_of_chars(pathname), flags, retval);
        return retval;
    }
}
