// AppImage-rs Type 2 runtime
// This is a rewrite of the Type 2 AppImage runtime in rust.

use std::{
    env,
    ffi::{CString, OsStr},
    fs::{self, File},
    os::fd::AsRawFd,
    path::Path,
    thread::{self},
};

const GIT_COMMIT: &str = ""; // TODO: Fix

fn appimage_get_elf_size(fname: &OsStr) -> anyhow::Result<u64> {
    let buffer = fs::read(fname)?;

    match goblin::Object::parse(&buffer)? {
        goblin::Object::Elf(elf) => {
            let e_shoff: u64 = elf.header.e_shoff;
            let e_shentsize: u64 = elf.header.e_shentsize.into();
            let e_shnum: u64 = elf.header.e_shnum.into();
            return Ok(e_shoff + (e_shentsize * e_shnum));
        }
        _ => {}
    }

    Err(anyhow::anyhow!(
        "Cannot get offset for AppImage at path {fname:?}"
    ))
}

fn print_help(appimage_path: &Path) {
    eprintln!(
        "AppImage options:
      --appimage-extract [<pattern>]  Extract content from embedded filesystem image
                                      If pattern is passed, only extract matching files
      --appimage-help                 Print this help
      --appimage-mount                Mount embedded filesystem image and print
                                      mount point and wait for kill with Ctrl-C
      --appimage-offset               Print byte offset to start of embedded
                                      filesystem image
      --appimage-portable-home        Create a portable home folder to use as $HOME
      --appimage-portable-config      Create a portable config folder to use as
                                      $XDG_CONFIG_HOME
      --appimage-signature            Print digital signature embedded in AppImage
      --appimage-updateinfo[rmation]  Print update info embedded in AppImage
      --appimage-version              Print version of AppImage runtime
    
    Portable home:
    
      If you would like the application contained inside this AppImage to store its
      data alongside this AppImage rather than in your home directory, then you can
      place a directory named
    
      {}.home
    
      Or you can invoke this AppImage with the --appimage-portable-home option,
      which will create this directory for you. As long as the directory exists
      and is neither moved nor renamed, the application contained inside this
      AppImage to store its data in this directory rather than in your home
      directory
    
    License: MIT License",
        appimage_path.display()
    );
}

fn portable_option(appimage_path: &Path, arg1: &OsStr, name: &str) -> anyhow::Result<bool> {
    if arg1.to_string_lossy() == format!("appimage-portable-{name}") {
        let path = format!("{}.{name}", appimage_path.to_string_lossy());
        let s = Path::new(&path);
        fs::create_dir(s)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn fusefs_main(offset: u64, mountpoint: &Path, appimage_path: &Path) -> anyhow::Result<()> {
    let fs = match squashfs_ng::read::Archive::open(appimage_path) {
        Ok(reader) => squashfuse_rs::SquashfsFilesystem::new(reader),
        Err(err) => {
            return Err(anyhow::anyhow!("Failed to open SquashFS image: {}", err));
        }
    };

    let mount_options = vec![
        fuser::MountOption::FSName("squashfuse".to_string()),
        fuser::MountOption::RO,
        fuser::MountOption::CUSTOM(format!("offset={offset}")),
    ];
    match fuser::mount2(fs, mountpoint, &mount_options) {
        Ok(()) => {
            println!("Mounted {:?} at {:?}", appimage_path, mountpoint);
            Ok(())
        }
        Err(err) => Err(anyhow::anyhow!("Failed to mount: {}", err)),
    }
}

fn main() -> anyhow::Result<()> {
    // We might want to operate on a target appimage rather than this file itself,
    // e.g., for appimaged which must not run untrusted code from random AppImages.
    // This variable is intended for use by e.g., appimaged and is subject to
    // change any time. Do not rely on it being present. We might even limit this
    // functionality specifically for builds used by appimaged.
    let (appimage_path, argv0_path, real_path) =
        if let Some(target_appimage) = env::var_os("TARGET_APPIMAGE") {
            (
                target_appimage.clone(),
                target_appimage.clone(),
                fs::canonicalize(target_appimage.clone()).unwrap(),
            )
        } else {
            (
                "/proc/self/exe".into(),
                env::args_os().next().expect("No argv[0]!"),
                fs::read_link("proc/self/exe").unwrap(),
            )
        };
    // get the ELF offset for the binary
    let fs_offset = appimage_get_elf_size(&appimage_path)?;
    // handle args
    if env::var_os("APPIMAGE_EXTRACT_AND_RUN").is_some() {
        // TODO: Implement
    } else if let Some(arg1) = env::args_os().nth(1) {
        if arg1 == OsStr::new("--appimage-help") {
            // HELP FUNCTION
            print_help(&real_path);
            return Ok(());
        }
        if arg1 == OsStr::new("--appimage-version") {
            // VERSION FUNCTION
            println!("{GIT_COMMIT}");
            return Ok(());
        }
        if arg1 == OsStr::new("--appimage-offset") {
            // OFFSET FUNCTION
            println!("{fs_offset}");
            return Ok(());
        }
        if arg1 == OsStr::new("--appimage-extract") {
            // EXTRACT FUNCTION
            // TODO: Implement
            return Ok(());
        }
        if arg1 == OsStr::new("--appimage-updateinformation")
            || arg1 == OsStr::new("appimage-updateinfo")
        {
            // TODO: Implement
            return Ok(());
        }
        if arg1 == OsStr::new("--appimage-signature") {
            // TODO: Implement
            return Ok(());
        }
        if arg1 == OsStr::new("--appimage-extract-and-run") {
            // TODO: Implement
        }
        // portable options
        if portable_option(&real_path, &arg1, "home")?
            || portable_option(&real_path, &arg1, "config")?
        {
            return Ok(());
        }
        // --appimage-mount is evaluated later, but other commands are not implemented
        if arg1 != OsStr::new("--appimage-mount")
            && arg1.to_string_lossy().starts_with("--appimage-")
        {
            return Err(anyhow::anyhow!(
                "{arg1:?} is not implemented in appimage version {GIT_COMMIT}"
            ));
        }
    }
    // make the temporary mountpoint
    const MAX_NAME_LENGTH: usize = 6;
    let temp_dir = tempfile::Builder::new()
        .prefix(&format!(
            ".mount_{}",
            real_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .chars()
                .take(MAX_NAME_LENGTH)
                .collect::<String>()
        ))
        .tempdir()?;
    let mount_dir = temp_dir.path().to_path_buf();
    let mount_dir_2 = mount_dir.clone();

    let (read_pipe, write_pipe) = rustix::pipe::pipe()?;
    // time to fork processes
    let real_path_2 = real_path.clone();
    let handle = thread::spawn(move || {
        // CHILD PROCESS
        unsafe {
            rustix::io::close(read_pipe.as_raw_fd());
        } // close read pipe. not sure why we need this?
        if let Err(e) = fusefs_main(fs_offset, &mount_dir_2, &real_path_2) {
            println!("Cannot mount AppImage, please check your FUSE setup.");
            println!(
                "You might still be able to extract the contents of this AppImage \
                    \nif you run it with the --appimage-extract option. \
                    \nSee https://github.com/AppImage/AppImageKit/wiki/FUSE \
                    \nfor more information"
            );
            println!("Error: {e}");
        }
        // TODO: fusefs stuff
    });
    // PARENT PROCESS. CHILD IS PID
    unsafe {
        rustix::io::close(write_pipe.as_raw_fd());
    } // close write pipe, not sure why we need this?
      // fuse process has now daemonized, reap our child
    handle.join().unwrap();

    let mount_f = File::open(&mount_dir)?;
    // TODO: Ask why we do this?
    let _res = nix::unistd::dup2(mount_f.as_raw_fd(), 1023)?;
    // Parse --appimage-mount arg
    if let Some(arg1) = env::args_os().nth(1) {
        if arg1 == OsStr::new("--appimage-mount") {
            if let Ok(canonical_dir) = mount_dir.canonicalize() {
                println!("{}", canonical_dir.to_string_lossy())
            } else {
                println!("{}", mount_dir.to_string_lossy())
            }
            loop {
                nix::unistd::pause()
            }
        }
    }
    // Setting some environment variables that the app "inside" might use
    env::set_var("APPIMAGE", &real_path);
    env::set_var("ARGV0", &argv0_path);
    env::set_var("APPDIR", &mount_dir);
    // If there is a directory with the same name as the AppImage plus ".home", then export $HOME
    if real_path.join(".home").exists() {
        eprintln!(
            "Setting $HOME to {}",
            real_path.join(".home").to_string_lossy()
        );
        env::set_var("HOME", real_path.join(".home"));
    }
    // If there is a directory with the same name as the AppImage plus ".config", then export $XDG_CONFIG_HOME
    if real_path.join(".config").exists() {
        eprintln!(
            "Setting $XDG_CONFIG_HOME to {}",
            real_path.join(".config").to_string_lossy()
        );
        env::set_var("XDG_CONFIG_HOME", real_path.join(".config"));
    }
    // set OWD
    env::set_var("OWD", env::current_dir()?);
    // Execute
    let exec_path = CString::new(mount_dir.join("AppRun").to_string_lossy().to_string())?;
    let argv = env::args_os()
        .map(|x| CString::new(x.to_string_lossy().to_string()).unwrap())
        .collect::<Vec<CString>>();
    // TODO: Find a way to get the exit status and/or output of this
    nix::unistd::execv(&exec_path, &argv.as_slice())?;
    // Error if we continue here
    Err(anyhow::anyhow!("execv error"))
}
