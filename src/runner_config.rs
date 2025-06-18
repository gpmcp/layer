use crate::retry_config::RetryConfig;
use derive_builder::Builder;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Default, Debug, Clone, PartialEq, Builder)]
#[builder(setter(into, strip_option))]
pub struct RunnerConfig {
    pub name: String,
    pub version: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub transport: Transport,
    pub working_directory: Option<PathBuf>,
    #[builder(default)]
    pub retry_config: RetryConfig,
}

impl RunnerConfig {
    pub fn builder() -> RunnerConfigBuilder {
        RunnerConfigBuilder::default()
    }
}

#[derive(Default, Debug, Clone, PartialEq)]
pub enum Transport {
    #[default]
    Stdio,
    Sse {
        url: String,
    },
}
