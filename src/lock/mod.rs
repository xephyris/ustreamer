use chrono::{DateTime, Utc};
use sysinfo::Process;

pub struct StreamLock {
    path: String,
}

impl StreamLock {
    pub fn aquire_lock(path: String) -> StreamLock {
        let pid = std::process::id();
        let timestamp = chrono::Utc::now().to_string();

        let lock_str = format!("{}\n{}", pid, timestamp);
        if std::path::Path::exists(&std::path::PathBuf::from(&path)) {
            let file = std::fs::read_to_string(&path).unwrap();
            let mut read = file.lines();
            println!("Old streamer found, killing streamer");
            if let Ok(old_timestamp) = read.next().unwrap_or("").to_owned().parse::<DateTime<Utc>>()
                && let Ok(old_pid) = read.next().unwrap_or("").to_owned().parse::<u32>() {
                if Utc::now() > old_timestamp {
                    if let Some(process) = sysinfo::System::new().process(sysinfo::Pid::from_u32(old_pid)) {
                        println!("Killing ustreamer at PID {}", old_pid);
                        sysinfo::Process::kill(process);
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
