use crate::runner::traits::{
    ProcessManager,
    TerminationStrategy,
};

/// Factory function to create the appropriate process manager for the current platform
pub fn create_platform_process_manager() -> Box<dyn ProcessManager> {
    #[cfg(unix)]
    {
        Box::new(crate::runner::platforms::unix::UnixProcessManager::new())
    }
    
    #[cfg(windows)]
    {
        Box::new(crate::runner::platforms::windows::WindowsProcessManager::new())
    }
    
    #[cfg(not(any(unix, windows)))]
    {
        compile_error!("Unsupported platform: only Unix and Windows are currently supported");
    }
}

/// Factory function to create the appropriate termination strategy for the current platform
pub fn create_platform_termination_strategy() -> Box<dyn TerminationStrategy> {
    #[cfg(unix)]
    {
        Box::new(crate::runner::platforms::unix::UnixTerminationStrategy::new_unix())
    }
    
    #[cfg(windows)]
    {
        Box::new(crate::runner::platforms::windows::WindowsTerminationStrategy::new_windows())
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
        let _termination_strategy = create_platform_termination_strategy();
    }
}