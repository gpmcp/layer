use gpmcp_layer_core::{ProcessManager, ProcessManagerFactory};

/// Platform-agnostic factory that selects the appropriate implementation at compile time
pub struct PlatformProcessManagerFactory;

impl ProcessManagerFactory for PlatformProcessManagerFactory {
    fn create_process_manager() -> Box<dyn ProcessManager> {
        #[cfg(unix)]
        {
            gpmcp_layer_unix::UnixProcessManagerFactory::create_process_manager()
        }

        #[cfg(windows)]
        {
            gpmcp_layer_windows::WindowsProcessManagerFactory::create_process_manager()
        }

        #[cfg(not(any(unix, windows)))]
        {
            compile_error!("Unsupported platform: only Unix and Windows are currently supported");
        }
    }

    fn platform_name() -> &'static str {
        #[cfg(unix)]
        {
            gpmcp_layer_unix::UnixProcessManagerFactory::platform_name()
        }

        #[cfg(windows)]
        {
            gpmcp_layer_windows::WindowsProcessManagerFactory::platform_name()
        }

        #[cfg(not(any(unix, windows)))]
        {
            "Unknown"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detection() {
        let platform = PlatformProcessManagerFactory::platform_name();
        println!("Running on platform: {}", platform);

        // Ensure we can create platform-specific managers
        let _process_manager = PlatformProcessManagerFactory::create_process_manager();
    }
}
