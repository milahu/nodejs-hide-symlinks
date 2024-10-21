/*
nodejs-hide-symlinks

hide symlinks from nodejs
to implement a symlinked machine-level global NPM store on nixos (and others)

license: MIT, author: milahu

based on
https://github.com/nodejs/node/blob/master/deps/uv/src/unix/linux-syscalls.c # uv__statx
https://github.com/geofft/redhook
https://github.com/abba23/spotify-adblock
https://doc.rust-lang.org/std/ffi/struct.CStr.html
https://users.rust-lang.org/t/intercepting-libc-readlink-with-a-rust-ld-preload-program-hangs-when-applying-to-cargo-build/48057
https://docs.rs/statx-sys/0.1.0/statx_sys/struct.statx.html
https://github.com/oxalica/statx-sys/blob/master/src/lib.rs
*/

// TODO silent. enable debug prints with env DEBUG=nodejs-hide-symlinks

extern crate libc;

#[macro_use]
extern crate redhook;

use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Arc;
use std::sync::Mutex;
use std::env;
//use colored::*; // "/".green()

struct PreloadState {
    map: HashMap<String, String>,
    init_done: bool,
    cwd: String // TODO keep track of cwd. hook chdir and syscall
}

lazy_static::lazy_static! {
    static ref PRELOAD_STATE: Arc<Mutex<PreloadState>> = {
        let cwd_str = std::env::current_dir().unwrap().into_os_string().into_string().unwrap(); // pathbuf to string
        let state = PreloadState {
            map: HashMap::new(),
            init_done: false,
            cwd: cwd_str
        };
        let state_locked = Arc::new(Mutex::new(state));
        return state_locked;
    };

    // TODO restore
    //static ref DEBUG: bool = env::var("NODEJS_HIDE_SYMLINKS_DEBUG").is_ok();
    static ref DEBUG: bool = true;
}



unsafe fn is_symlink(statxbuf: *const libc::statx) -> bool {
    // https://man7.org/linux/man-pages/man7/inode.7.html
    const S_IFMT:   u16 = 0o170000; // bit mask for the file type bit field
    const S_IFLNK:  u16 = 0o120000; // symbolic link
    return ((*statxbuf).stx_mode & S_IFMT) == S_IFLNK;
}



/*
            // todo? simplify cast from libc::ssize_t to libc::c_long

            //return statx_hooked(dirfd, path, flags, mask, statxbuf).into();
            return statx_hooked(dirfd, path, flags, mask, statxbuf) as libc::c_long;

            // error: the trait `From<isize>` is not implemented for `i64`, which is required by `isize: Into<_>`
            //return readlink_hooked(path, buf, bufsz).into();
            // error: expected `i64`, found `isize`
            //return readlink_hooked(path, buf, bufsz);
            // error: type annotations needed
            //return readlink_hooked(path, buf, bufsz).into() as libc::c_long;
            // the trait `From<isize>` is not implemented for `i64`, which is required by `isize: Into<i64>`
            //return <libc::ssize_t as Into<libc::c_long>>::into(readlink_hooked(path, buf, bufsz).into());
            return readlink_hooked(path, buf, bufsz) as libc::c_long;
*/



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
            println!("hooking syscall {}", num);
            let dirfd = a1 as libc::c_int;
            let path = a2 as *const libc::c_char;
            let flags = a3 as libc::c_int;
            let mask = a4 as libc::c_uint;
            let statxbuf = a5 as *mut libc::statx;
            return statx_hooked(dirfd, path, flags, mask, statxbuf) as libc::c_long;
        }

        if num == libc::SYS_readlink {
            println!("hooking syscall {}", num);
            let path = a1 as *const libc::c_char;
            let buf = a2 as *mut libc::c_char;
            let bufsz = a3 as libc::size_t;
            return readlink_hooked(path, buf, bufsz) as libc::c_long;
        }

        if num == libc::SYS_readlinkat {
            println!("hooking syscall {}", num);
            let dirfd = a1 as libc::c_int;
            let pathname = a2 as *const libc::c_char;
            let buf = a3 as *mut libc::c_char;
            let bufsz = a4 as libc::size_t;
            return readlinkat_hooked(dirfd, pathname, buf, bufsz) as libc::c_long;
        }

        // TODO handle libc::SYS_chdir

        println!("ignoring syscall {}", num);
        return real!(syscall)(num, a1, a2, a3, a4, a5);
    }
}



hook! {
    unsafe fn chdir(
        dir: *const libc::c_char
    ) -> libc::c_int => my_chdir {

        let retval = real!(chdir)(dir);
        let dir_str = str_of_chars(dir);
        if retval == 0 {
            // success
            let mut preload_state = PRELOAD_STATE.lock().unwrap();
            if dir_str.starts_with("/") {
                preload_state.cwd = dir_str.to_owned();
            }
            else {
                // get absolute path
                // TODO better (resolve ../)
                let mut dir_abs = preload_state.cwd.to_owned();
                dir_abs.push_str("/");
                dir_abs.push_str(dir_str);
                preload_state.cwd = dir_abs;
            }
            if *DEBUG {
                eprintln!("nodejs-hide-symlinks chdir {}", preload_state.cwd);
            }
        }
        return retval;
    }
}



hook! {
    unsafe fn open(
        path: *const libc::c_char, 
        oflag: libc::c_int
    ) -> libc::c_int => my_open {

        let path_str = str_of_chars(path);

        if *DEBUG {
            eprintln!("nodejs-hide-symlinks open(\"{}\", {})", path_str, oflag);
        }

        /*
        let symlink_map_mutex = Arc::clone(&SYMLINK_MAP);
        let symlink_map = symlink_map_mutex.lock().unwrap();
        */

        //let mut preload_state = PRELOAD_STATE.lock().unwrap();
        let preload_state = PRELOAD_STATE.lock().unwrap();
        //

        // find longest prefix
        let mut path_prefix: &String = &String::new(); // TODO better?
        //for cur_path_prefix in symlink_map.keys() {
        for cur_path_prefix in preload_state.map.keys() {
            if !path_str.starts_with(cur_path_prefix) {
                continue;
            }
            if path_prefix.len() < cur_path_prefix.len() {
                path_prefix = cur_path_prefix;
            }
        }

        if path_prefix.len() == 0 {
            // noop
            let retval = real!(open)(path, oflag);
            return retval;
        }

        // rewrite path
        //let mut path_resolved_str = symlink_map.get(path_prefix).unwrap().to_owned(); // get resolved prefix
        let mut path_resolved_str = preload_state.map.get(path_prefix).unwrap().to_owned(); // get resolved prefix
        path_resolved_str.push_str(&path_str[path_prefix.len()..]);

        // TODO use state.cwd
        let cwd_str = std::env::current_dir().unwrap().into_os_string().into_string().unwrap(); // pathbuf to string
        if *DEBUG {
            //eprintln!("nodejs-hide-symlinks open(\"{}\", {})", path_str, oflag);
            eprintln!("nodejs-hide-symlinks open: wrong cwd   \"{}\"", cwd_str);
            eprintln!("nodejs-hide-symlinks open: path_prefix \"{}\"", path_prefix);
            eprintln!("nodejs-hide-symlinks open: path        \"{}\"", path_str);
            /*
            // FIXME thread '<unnamed>' panicked at 'byte index 51 is out of bounds of `/home/user/src/milahu/nur-packages/result`', src/lib.rs:187:27
            let prefix_rel = &path_prefix[(cwd_str.len() + 1)..];
            let path_rel = &path_str[(path_prefix.len() + 1)..];
            eprintln!("nodejs-hide-symlinks open {}{}{}", prefix_rel, "/".green(), path_rel);
            */
        }

        // BROKEN
        //let path_resolved = chars_of_str(&path_resolved_str.to_owned());

        // TODO move to macro
        let r_string_2 = path_resolved_str.to_owned();
        let c_string_2 = CString::new(r_string_2).unwrap();
        let path_resolved: *const libc::c_char = c_string_2.as_ptr() as *const libc::c_char;

        /*
        let r_string = "open: path_resolved = \"%s\"\n";
        let c_string = CString::new(r_string).unwrap();
        let libc::c_chars: *const libc::c_char = c_string.as_ptr() as *const libc::c_char;
        libc::printf(c_chars, path_resolved); // test *const libc::c_char
        */

        let retval = real!(open)(path_resolved, oflag);
        //println!("open({}) -> retval {}", path_resolved_str, retval);
        return retval;
    }
}



// statx is not called by node (never?)
// node calls syscall(SYS_statx, ...)
// https://stackoverflow.com/questions/49314057/how-to-find-out-what-functions-to-intercept-with-ld-preload

hook! {
    unsafe fn statx(
        dirfd: libc::c_int,
        path: *const libc::c_char,
        flags: libc::c_int,
        mask: libc::c_uint,
        statxbuf: *mut libc::statx // https://docs.rs/libc/0.2.103/libc/struct.statx.html
    ) -> libc::c_int => statx_hooked {

        let mut retval = real!(statx)(dirfd, path, flags, mask, statxbuf);

        if *DEBUG {
            //println!("nodejs-hide-symlinks syscall(SYS_statx, dir {}, path {}, flags {}, mask {}) -> retval {:?}", dirfd, path_str, flags, mask, retval);
            //println!("nodejs-hide-symlinks statx -> stx_mode: {:#o}, symlink: {:?}", (*statxbuf).stx_mode, is_symlink(statxbuf));
            let path_str = str_of_chars(path);
            let statxbuf_str = string_of_statxbuf(statxbuf);
            eprintln!("nodejs-hide-symlinks a: statx({}, \"{}\", {}, {}, &statxbuf) -> retval: {:?}, statxbuf: {}", dirfd, path_str, flags, mask, retval, statxbuf_str);
        }

        if !(flags == 256 && is_symlink(statxbuf)) {
            // only with flags == 256, symlinks are detected
            return retval;
        }

        let path_str = str_of_chars(path);

        if *DEBUG {
            //println!("nodejs-hide-symlinks syscall(SYS_statx, dir {}, path {}, flags {}, mask {}) -> retval {:?}", dirfd, path_str, flags, mask, retval);
            //println!("nodejs-hide-symlinks statx -> stx_mode: {:#o}, symlink: {:?}", (*statxbuf).stx_mode, is_symlink(statxbuf));
            let statxbuf_str = string_of_statxbuf(statxbuf);
            eprintln!("nodejs-hide-symlinks a: statx({}, \"{}\", {}, {}, &statxbuf) -> retval: {:?}, statxbuf: {}", dirfd, path_str, flags, mask, retval, statxbuf_str);
        }

        // resolve symlink
        // TODO cache
        const BUFSIZ: libc::size_t = 8191;
        let mut buf_array = [(0 as libc::c_char); BUFSIZ];
        let buf = buf_array.as_mut_ptr(); // *mut libc::c_char;
        //let buf_len = real!(readlink)(path, buf, BUFSIZ); // error: readlink not found in this scope (no hook for readlink)
        let buf_len = real!(syscall)(libc::SYS_readlink,
            path as *mut libc::c_void,
            buf as *mut libc::c_void,
            BUFSIZ as *mut libc::c_void,
            std::ptr::null_mut() as *mut libc::c_void,
            std::ptr::null_mut() as *mut libc::c_void
        );
        /*
        if *DEBUG {
            let buf_str = str_of_chars(buf);
            eprintln!("nodejs-hide-symlinks readlink({}) -> buf {}, buf_len {:?}", str_of_chars(path), buf_str, buf_len);
        }
        */
        // handle links to /nix/store
        const PREFIX_MIN_LEN: libc::c_long = 45; // string length of /nix/store/00000000000000000000000000000000-x
        if buf_len < PREFIX_MIN_LEN { // micro optimization
            return retval;
        }

        let link_target = str_of_chars(buf);
        if !link_target.starts_with("/nix/store/") {
            return retval;
        }

        // we have a winner! found symlink to /nix/store
        // -> hide the symlink, tell node it's a real file

        let mut preload_state = PRELOAD_STATE.lock().unwrap();

        if !preload_state.init_done {
            // init
            // print the first cwd
            if *DEBUG {
                eprintln!("nodejs-hide-symlinks init {}", preload_state.cwd);
            }
            preload_state.init_done = true;
        }

        // TODO get relative path, rel to cwd
        // TODO use state.cwd
        let cwd_str = std::env::current_dir().unwrap().into_os_string().into_string().unwrap(); // pathbuf to string

        if *DEBUG {
            eprintln!("nodejs-hide-symlinks wrong cwd   \"{}\"", cwd_str);
            eprintln!("nodejs-hide-symlinks path        \"{}\"", path_str);
            /*
            let path_str_cut = cwd_str.len() + 1;
            // FIXME thread '<unnamed>' panicked at 'byte index 51 is out of bounds of `/home/user/src/milahu/nur-packages/result`', src/lib.rs:287:29
            let path_str_end = &path_str[path_str_cut..];
            eprintln!("nodejs-hide-symlinks stat {}{}", path_str_end, "/".green()); // assert: path is directory
            */
        }

        let path_resolved_str = std::fs::canonicalize(link_target).unwrap().into_os_string().into_string().unwrap(); // pathbuf to string

        // TODO move to macro
        let r_string_2 = path_resolved_str.to_owned();
        let c_string_2 = CString::new(r_string_2).unwrap();
        let path_resolved: *const libc::c_char = c_string_2.as_ptr() as *const libc::c_char;

        /*
        let r_string = "statx: path_resolved = \"%s\"\n";
        let c_string = CString::new(r_string).unwrap();
        let libc::c_chars: *const libc::c_char = c_string.as_ptr() as *const libc::c_char;
        libc::printf(c_chars, path_resolved); // test *const libc::c_char
        */

        let path_str = str_of_chars(path);
        //println!("readlink(...) -> path_resolved {:?}", path_resolved_str);
        preload_state.map.insert(path_str.to_owned(), path_resolved_str.to_owned());
        drop(preload_state); // unlock early

        // preserve the inode number
        let stx_ino = (*statxbuf).stx_ino;

        // overwrite statxbuf
        *statxbuf = std::mem::zeroed();
        //retval = real!(statx)(dirfd, path_resolved, flags, mask, statxbuf) as libc::c_long;
        retval = real!(statx)(dirfd, path_resolved, flags, mask, statxbuf);
        // path is absolute (starts with slash), so dirfd is ignored
        //println!("statx({}) retval: {}", path_resolved_str, retval);
        //println!("statx({}) stx_mode: {:#o}", path_resolved_str, (*statxbuf).stx_mode);

        // TODO remove
        // restore the inode number
        (*statxbuf).stx_ino = stx_ino;

        if *DEBUG {
            //let retval = real!(statx)(dirfd, path, flags, mask, statxbuf);
            //let path_str = str_of_chars(path);
            let path_resolved_str = str_of_chars(path_resolved);
            let statxbuf_str = string_of_statxbuf(statxbuf);
            eprintln!("nodejs-hide-symlinks b: statx({}, \"{}\", {}, {}, &statxbuf) -> retval: {:?}, statxbuf: {}", dirfd, path_resolved_str, flags, mask, retval, statxbuf_str);
            //eprintln!("nodejs-hide-symlinks hiding {}", path_resolved_str);
            //println!("statx() stx_mode: {:#o}", (*statxbuf).stx_mode);
        }

        return retval;
    }
}



// "npx vite" seems to bypass statx and call readlink directly

hook! {
    // https://docs.rs/libc/latest/libc/fn.readlink.html
    unsafe fn readlink(
        path: *const libc::c_char,
        buf: *mut libc::c_char,
        bufsz: libc::size_t
    ) -> libc::ssize_t => readlink_hooked {
        let mut retval = real!(readlink)(path, buf, bufsz);

        if *DEBUG {
            eprintln!("nodejs-hide-symlinks a: readlink(\"{}\", &buf, bufsz) -> retval: {:?}, buf: {}", str_of_chars(path), retval, str_of_chars(buf));
        }

        // TODO modify retval and *buf

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

        if *DEBUG {
            eprintln!("nodejs-hide-symlinks a: readlinkat({}, \"{}\", &buf, bufsz) -> retval: {:?}, buf: {}", dirfd, str_of_chars(pathname), retval, str_of_chars(buf));
        }

        // TODO modify retval and *buf

        return retval;
    }
}



// debug
hook! {
    // https://docs.rs/libc/latest/libc/fn.openat.html
    unsafe fn openat(
        dirfd: libc::c_int,
        pathname: *const libc::c_char,
        flags: libc::c_int
    ) -> libc::c_int => openat_hooked {
        let mut retval = real!(openat)(dirfd, pathname, flags);

        if *DEBUG {
            eprintln!("nodejs-hide-symlinks a: openat({}, \"{}\", {}) -> retval: {:?}", dirfd, str_of_chars(pathname), flags, retval);
        }

        // TODO modify retval

        return retval;
    }
}



unsafe fn str_of_chars(cstr: *const libc::c_char) -> &'static str {
    return std::str::from_utf8(std::ffi::CStr::from_ptr(cstr).to_bytes()).unwrap();
}

unsafe fn string_of_statxbuf(statxbuf: *const libc::statx) -> String {
    return format!("{{ stx_ino: {}, stx_mode: {:#o}, is_symlink: {:?} }}", (*statxbuf).stx_ino, (*statxbuf).stx_mode, is_symlink(statxbuf));
}

/*
// https://users.rust-lang.org/t/how-to-convert-a-non-zero-terminated-c-string-to-rust-str-or-string/2177
unsafe fn str_of_chars_size(cstr: *const libc::c_char, size: isize) -> &'static str {
    return std::str::from_utf8(std::slice::from_raw_parts(cstr as *const u8, size as usize)).unwrap()
}
*/



// https://doc.rust-lang.org/std/ffi/struct.CString.html
// type libc::c_char = i8;

/* BROKEN
fn chars_of_str(string: &str) -> *const libc::c_char {
    let cstring = std::ffi::CString::new(string).unwrap();
    return cstring.as_ptr();
}
*/


