use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::broadcast::Sender;

use super::{emit, DanmakuSource};
use crate::models::DanmakuMessage;

pub struct XhsSource;

#[async_trait]
impl DanmakuSource for XhsSource {
    async fn run(self: Box<Self>, tx: Sender<DanmakuMessage>) -> Result<()> {
        emit(&tx, DanmakuMessage::info("小红书暂不支持弹幕，可在设置中关闭弹幕显示"));
        Ok(())
    }
}
