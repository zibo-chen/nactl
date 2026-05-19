use crate::application::service::NactlApplicationService;
use crate::error::AppResult;
use crate::infrastructure::config::{ConfigOverrides, RuntimeConfig};
use crate::interface::cli::GlobalArgs;

pub struct RuntimeBootstrap {
    pub config: RuntimeConfig,
    pub service: NactlApplicationService,
}

impl RuntimeBootstrap {
    pub fn from_global_args(global: &GlobalArgs) -> AppResult<Self> {
        let overrides = ConfigOverrides {
            server: global.server.clone(),
            context_path: global.context_path.clone(),
            namespace: global.namespace.clone(),
            username: global.username.clone(),
            password: global.password.clone(),
            access_token: global.access_token.clone(),
            config: global.config.clone(),
            timeout_secs: global.timeout_secs,
            verbose: global.verbose,
        };
        let config = RuntimeConfig::resolve(overrides)?;
        let service = NactlApplicationService::from_runtime(config.clone())?;
        Ok(Self { config, service })
    }
}
