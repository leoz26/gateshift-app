use super::NetworkPlatform;
use crate::error::{Error, Result};
use crate::model::{DnsConfig, NetworkInterface};

/// 其余平台：所有操作都明确报错，而不是静默什么都不做。
pub struct UnsupportedPlatform;

impl NetworkPlatform for UnsupportedPlatform {
    fn list_interfaces(&self) -> Result<Vec<NetworkInterface>> {
        Err(Error::UnsupportedPlatform)
    }

    fn default_interface(&self) -> Result<NetworkInterface> {
        Err(Error::UnsupportedPlatform)
    }

    fn get_gateway(&self, _: &NetworkInterface) -> Result<Option<String>> {
        Err(Error::UnsupportedPlatform)
    }

    fn set_gateway(&self, _: &NetworkInterface, _: &str) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }

    fn get_dns(&self, _: &NetworkInterface) -> Result<DnsConfig> {
        Err(Error::UnsupportedPlatform)
    }

    fn set_dns(&self, _: &NetworkInterface, _: &[String]) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }

    fn flush_dns(&self) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
}
