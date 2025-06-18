use super::process_traits::ProcessManager;

#[cfg(unix)]
use super::unix_process_manager::UnixProcessManager;
#[cfg(windows)]
use super::windows_process_manager::WindowsProcessManager;

/// Factory function to create the appropriate process manager for the current platform
pub fn create_platform_process_manager() -> Box<dyn ProcessManager> {
    #[cfg(unix)]
    {
        Box::new(UnixProcessManager::new())
    }

    #[cfg(windows)]
    {
        Box::new(WindowsProcessManager::new())
    }

    #[cfg(not(any(unix, windows)))]
    {
        compile_error!("Unsupported platform: only Unix and Windows are currently supported");
    }
}

/// Get the platform name for logging and debugging
pub fn platform_name() -> &'static str {
    #[cfg(unix)]
    {
        "Unix"
    }

    #[cfg(windows)]
    {
        "Windows"
    }

    #[cfg(not(any(unix, windows)))]
    {
        "Unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detection() {
        let platform = platform_name();
        println!("Running on platform: {}", platform);

        // Ensure we can create platform-specific managers
        let _process_manager = create_platform_process_manager();
    }
}
