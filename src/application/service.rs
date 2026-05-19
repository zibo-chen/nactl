use crate::error::AppResult;
use crate::infrastructure::config::RuntimeConfig;
use crate::infrastructure::nacos::{
    ConfigGetResult, ConfigListResult, ConfigSetRequest, LoginResult, NacosOpenApiClient,
    SearchMode,
};

pub struct NactlApplicationService {
    client: NacosOpenApiClient,
}

impl NactlApplicationService {
    pub fn from_runtime(config: RuntimeConfig) -> AppResult<Self> {
        Ok(Self {
            client: NacosOpenApiClient::new(config)?,
        })
    }

    pub async fn login(&mut self) -> AppResult<LoginResult> {
        self.client.login().await
    }

    pub async fn list_configs(
        &mut self,
        data_id: Option<&str>,
        group: Option<&str>,
        page: usize,
        size: usize,
        search_mode: SearchMode,
    ) -> AppResult<ConfigListResult> {
        self.client
            .list_configs(data_id, group, page, size, search_mode)
            .await
    }

    pub async fn get_config(
        &mut self,
        data_id: &str,
        group: &str,
    ) -> AppResult<Option<ConfigGetResult>> {
        self.client.get_config(data_id, group).await
    }

    pub async fn set_config(&mut self, request: &ConfigSetRequest) -> AppResult<()> {
        self.client.set_config(request).await
    }

    pub async fn remove_config(&mut self, data_id: &str, group: &str) -> AppResult<()> {
        self.client.remove_config(data_id, group).await
    }
}
