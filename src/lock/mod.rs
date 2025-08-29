use chrono::{DateTime, Utc};
use sysinfo::Process;

pub struct StreamLock {
    path: String,
}

impl StreamLock {
    pub fn aquire_lock(path: String) -> StreamLock {
        let pid = std::process::id();
        let target_name = "ustreamer";

        let lock_str = format!("{}\n", pid);
        if std::path::Path::exists(&std::path::PathBuf::from(&path)) {
            let file = std::fs::read_to_string(&path).unwrap();
            let mut read = file.lines();
            println!("Old streamer found, killing streamer");
            if let Ok(old_pid) = read.next().unwrap_or("").to_owned().parse::<u32>() {
                let mut system = sysinfo::System::new_all();
                system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

               for (pid, process) in system.processes() {
                    if process.name() == target_name && pid.as_u32() != std::process::id(){
                        println!("Killing '{}' at PID {}", target_name, pid);
                        process.kill();
                    }
                }
            }
        }
        println!("Writing to lock file");
        if let Err(e) = std::fs::write(&path, lock_str) {
            println!("Lock failed with error: {}", e);
        }

        

        StreamLock {
            path
        }
    }
}

impl Drop for StreamLock {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.path) {
            eprintln!("Failed to remove lock with error: {e}");
        }
    }
}
