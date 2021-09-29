# nodejs-hide-symlinks

hide symlinks from nodejs,
to implement a symlinked machine-level global NPM store on nixos (and others)

## build

this requires `rust nightly`, as specified in [rust-toolchain.toml](rust-toolchain.toml).
`rust stable` will silently fail ...

```
cargo build
```

## use

```
LD_PRELOAD="target/debug/libnodejs_hide_symlinks.so" node
```

in most cases, this will do nothing

the `libnodejs_hide_symlinks.so` program will only become active,  
when `require` or `import` try to resolve a symlink to `/nix/store/`

then, the `statx` syscall is intercepted,  
so that the symlink-source appears as a regular directory (or file)

the following `open` syscalls are intercepted,  
so that `open(symlink-source)` is replaced with `open(symlink-target)`

### sample output


with `libnodejs_hide_symlinks.so`

<pre>

<span color="turquoise">$</span> pwd

/tmp/test-project

<span color="turquoise">$</span> LD_PRELOAD=/tmp/nodejs-hide-symlinks/target/debug/libnodejs_hide_symlinks.so \
node node_modules/cowsay/cli.js moooooh

nodejs-hide-symlinks init /tmp/test-project
nodejs-hide-symlinks stat node_modules<span color="green">/</span>
nodejs-hide-symlinks stat node_modules/.pnpm/cowsay@1.5.0/node_modules/cowsay<span color="green">/</span>
nodejs-hide-symlinks open node_modules/.pnpm/cowsay@1.5.0/node_modules/cowsay<span color="green">/</span>package.json
nodejs-hide-symlinks open node_modules/.pnpm/cowsay@1.5.0/node_modules/cowsay<span color="green">/</span>cli.js
nodejs-hide-symlinks open node_modules<span color="green">/</span>.pnpm/cowsay@1.5.0/node_modules/yargs/package.json
nodejs-hide-symlinks stat node_modules/.pnpm/yargs@15.4.1/node_modules/yargs<span color="green">/</span>
nodejs-hide-symlinks open node_modules/.pnpm/yargs@15.4.1/node_modules/yargs<span color="green">/</span>index.js
[...]
nodejs-hide-symlinks open node_modules/.pnpm/cowsay@1.5.0/node_modules/cowsay<span color="green">/</span>lib/cows.js
nodejs-hide-symlinks open node_modules/.pnpm/cowsay@1.5.0/node_modules/cowsay<span color="green">/</span>lib/replacer.js
nodejs-hide-symlinks open node_modules/.pnpm/cowsay@1.5.0/node_modules/cowsay<span color="green">/</span>lib/faces.js
nodejs-hide-symlinks open node_modules/.pnpm/cowsay@1.5.0/node_modules/cowsay<span color="green">/</span>cows/default.cow
 _________
&lt; moooooh &gt;
 ---------
        \   ^__^
         \  (oo)\_______
            (__)\       )\/\
                ||----w |
                ||     ||

</pre>

left of the green slashes, are the "fake directories",
which are transparently resolved into the `/nix/store`

for example

```
$ readlink node_modules/.pnpm/cowsay@1.5.0/node_modules/cowsay

/nix/store/4qjr3svb3dmmq2b2yn69y3wlz453wldn-cowsay-1.5.0.tgz-unpacked
```

### sample error

all this is needed, because nodejs would follow the symlink into the `/nix/store`,  
but there, the package's sub-dependencies cannot be located

without `libnodejs_hide_symlinks.so`

```
$ node node_modules/cowsay/cli.js moooooh

node:internal/modules/cjs/loader:936
  throw err;
  ^

Error: Cannot find module 'yargs'
Require stack:
- /nix/store/4qjr3svb3dmmq2b2yn69y3wlz453wldn-cowsay-1.5.0.tgz-unpacked/cli.js
```

### symlinked node_modules

we stop the symlink-resolution one step before the `/nix/store`,  
so that nodejs can resolve sub-dependencies in the local `node_modules` folder

```
$ ls -l -a node_modules/

dr-xr-xr-x 35 root root 4096 Jan  1  1970 .pnpm
lrwxrwxrwx  1 root root   38 Jan  1  1970 cowsay -> .pnpm/cowsay@1.5.0/node_modules/cowsay

$ tree node_modules/.pnpm/ | head -n7

node_modules/.pnpm/
├── ansi-regex@3.0.0
│   └── node_modules
│       └── ansi-regex -> /nix/store/a17g3kl3bb4gmwzjw9s9k4sz8k0zh4jx-ansi-regex-3.0.0.tgz-unpacked
├── ansi-regex@5.0.1
│   └── node_modules
│       └── ansi-regex -> /nix/store/xxmkcs4fyl4by41a9vpf8zanad9xj3pr-ansi-regex-5.0.1.tgz-unpacked
```

this custom node_modules folder can be generated  
with a patched version of [npmlock2nix](https://github.com/nix-community/npmlock2nix)  
and with the custom NPM installer [pnpm-install-only](https://github.com/milahu/pnpm-install-only)

```nix
# internal.nix

rec {

  # add
  unpackNpmTgz = { url, hash }:
    stdenv.mkDerivation {
      #name = builtins.elemAt (builtins.match "^(.+)\.tgz$" (builtins.baseNameOf url)) 0;
      name = "${builtins.baseNameOf url}-unpacked";
      src = fetchurl { inherit url hash; }; # cache the *.tgz file in /nix/store
      phases = "unpackPhase installPhase";
      installPhase = ''
        cd ..
        mv package $out
      '';
    };

  # replace fetchurl with unpackNpmTgz
  makeSource = sourceHashFunc: name: dependency:
    # ...
    if dependency ? resolved && dependency ? integrity then
      #dependency // { resolved = "file://" + (toString (fetchurl (makeSourceAttrs name dependency))); }
      dependency // { resolved = "file://" + (toString (unpackNpmTgz (makeSourceAttrs name dependency))); }
    else /* ... */ null;

  # add
  pnpm_install_only = fetchFromGitHub {
    # https://github.com/milahu/pnpm-install-only
    repo = "pnpm-install-only";
    owner = "milahu";
    rev = "TODO";
    sha256 = "TODO";
  };

  # in the buildPhase of node_modules, replace "npm install"
  node_modules__buildPhase = ''
          #npm install --offline --nodedir=${nodeSource nodejs}
          
          export NODE_preInstallLinks='${builtins.toJSON preInstallLinks}'
          node --trace-uncaught --trace-warnings ${pnpm_install_only}/dist/index.js || {
            echo "ERROR failed to install NPM packages"
            exit 1
          }
        '';
}
```

## related

unmerged PR at https://github.com/nodejs/node/pull/10132

https://github.com/nodejs/node/issues/10107

https://github.com/nodejs/node-eps/issues/46#issuecomment-265227695

> Using Machine Level stores while keep dependency version resolution coupled to a given /node_modules root.

https://github.com/nodejs/node-eps/issues/46#issuecomment-266173249

> > When a module is found, its node_modules hierarchy includes `<path>/node_modules` as well as `<path>+node_modules` for all the elements in its path, starting with the path.dirname() of where it is found.
>
> Correct. However, I consider this the anm enhancement, and an entirely separate thing from 1 & 2, which are just about working with symlinks generally. anm has no idea if symlinks are involved or not, it just makes using them to machine stores possible. They really should be seen is two discreet things.

https://github.com/nodejs/node-eps/issues/46#issuecomment-266635299

> symlinks per se aren't needed to pull off machine-level stores. For instance, it would be an acceptable compromise to wrap commands with a shim, because npm already does this on Windows. If we're going to need to wrap top-level applications in a shim regardless, we might as well use that shim to monkey-patch module.require. Maybe everyone will boo and hiss at that idea, but I'm really a fan of keeping core lean, and being able to solve machine-level stores purely with userspace code appeals to me. Thoughts?

https://github.com/nodejs/node-eps/issues/46#issuecomment-267643146

> > Just to be clear, I mean this warning in the sense of "That is going to be an exciting adventure!", and I strongly encourage you to try it, if you've got the time and are in the mood for adventure. But it definitely won't be safe, so it shouldn't be something that npm or node-core try to do :)
>
> Thanks! And yes, it's precisely because it is crazy and out there that I'm going to try to do it in user space via monkey patching rather than by branching node core. It'll be fun. When/if I get a machine-store loader working I'll post back with a link.

https://github.com/nodejs/node-eps/issues/46#issuecomment-277373566

<blockquote>

The latest version of pnpm (which is 0.51.2) uses a global (machine) store and works without any changes in Node.js.

We did a lot of tweaks to make it work, but the main ones are:

* files are linked (not symlinked) from the global store to the project's node_modules
* command shims are rewritten to set the NODE_PATH env variable before running the binstubs.

So it is achievable to create a global store without changes in Node.js and without --preserve-symlinks. And performance is good enough with linking.

</blockquote>
