//! MM2 process manager with lifecycle and health monitoring

use crate::{binary::Mm2Binary, config::Mm2Config, client::Mm2Client, Result, Error};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{info, warn, error, debug};

/// MM2 process state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mm2State {
    /// Not started
    Stopped,
    /// Starting up
    Starting,
    /// Running and healthy
    Running,
    /// Running but unhealthy
    Unhealthy,
    /// Stopping
    Stopping,
    /// Failed
    Failed,
}

/// MM2 health status
#[derive(Debug, Clone)]
pub struct Mm2Health {
    /// Current state
    pub state: Mm2State,
    /// Last successful health check
    pub last_check: Option<std::time::SystemTime>,
    /// RPC reachable
    pub rpc_reachable: bool,
    /// Enabled coins count
    pub enabled_coins: usize,
    /// Uptime in seconds
    pub uptime_secs: u64,
}

/// MM2 process manager with lifecycle and health monitoring
pub struct Mm2Manager {
    binary: Mm2Binary,
    config: Mm2Config,
    state: Arc<RwLock<Mm2State>>,
    process: Arc<RwLock<Option<Child>>>,
    client: Arc<Mm2Client>,
    start_time: Arc<RwLock<Option<std::time::SystemTime>>>,
}

impl Mm2Manager {
    /// Create new manager
    pub fn new(binary_path: PathBuf, config: Mm2Config) -> Self {
        let client = Arc::new(Mm2Client::new(
            format!("http://127.0.0.1:{}", config.rpc_port),
            config.userpass.clone(),
        ));

        Self {
            binary: Mm2Binary::new(binary_path),
            config,
            state: Arc::new(RwLock::new(Mm2State::Stopped)),
            process: Arc::new(RwLock::new(None)),
            client,
            start_time: Arc::new(RwLock::new(None)),
        }
    }

    /// Get current state
    pub async fn state(&self) -> Mm2State {
        *self.state.read().await
    }

    /// Get health status
    pub async fn health(&self) -> Mm2Health {
        let state = *self.state.read().await;
        let rpc_reachable = self.check_rpc_reachable().await;
        let enabled_coins = self.get_enabled_coins_count().await;
        
        let uptime_secs = if let Some(start) = *self.start_time.read().await {
            start.elapsed().map(|d| d.as_secs()).unwrap_or(0)
        } else {
            0
        };

        Mm2Health {
            state,
            last_check: Some(std::time::SystemTime::now()),
            rpc_reachable,
            enabled_coins,
            uptime_secs,
        }
    }

    /// Start MM2 process
    pub async fn start(&self) -> Result<()> {
        let current_state = *self.state.read().await;
        
        if current_state != Mm2State::Stopped {
            return Err(Error::Manager(format!(
                "Cannot start MM2 in state {:?}",
                current_state
            )));
        }

        *self.state.write().await = Mm2State::Starting;
        info!("Starting MM2 on port {}...", self.config.rpc_port);

        // Check binary exists
        if !self.binary.exists() {
            *self.state.write().await = Mm2State::Failed;
            return Err(Error::Manager("MM2 binary not found".to_string()));
        }

        // Write config file
        let config_path = self.write_config_file().await?;

        // Spawn MM2 process
        let child = Command::new(self.binary.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Manager(format!("Failed to spawn MM2: {}", e)))?;

        info!("MM2 process spawned (PID: {})", child.id());

        *self.process.write().await = Some(child);
        *self.start_time.write().await = Some(std::time::SystemTime::now());

        // Wait for MM2 to be ready
        self.wait_for_ready().await?;

        *self.state.write().await = Mm2State::Running;
        info!("MM2 is running and ready");

        Ok(())
    }

    /// Stop MM2 process
    pub async fn stop(&self) -> Result<()> {
        let current_state = *self.state.read().await;
        
        if current_state == Mm2State::Stopped {
            return Ok(());
        }

        *self.state.write().await = Mm2State::Stopping;
        info!("Stopping MM2...");

        // Try graceful shutdown via RPC
        if let Err(e) = self.client.stop().await {
            warn!("Graceful shutdown failed: {}", e);
        }

        // Kill process if still running
        if let Some(mut child) = self.process.write().await.take() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    info!("MM2 exited with status: {}", status);
                }
                Ok(None) => {
                    warn!("MM2 still running, killing...");
                    if let Err(e) = child.kill() {
                        error!("Failed to kill MM2: {}", e);
                    }
                }
                Err(e) => {
                    error!("Error checking MM2 status: {}", e);
                }
            }
        }

        *self.state.write().await = Mm2State::Stopped;
        *self.start_time.write().await = None;
        info!("MM2 stopped");

        Ok(())
    }

    /// Restart MM2 process
    pub async fn restart(&self) -> Result<()> {
        info!("Restarting MM2...");
        self.stop().await?;
        
        // Wait a bit before restarting
        tokio::time::sleep(Duration::from_secs(2)).await;
        
        self.start().await
    }

    /// Start health monitoring loop
    pub async fn start_health_monitor(self: Arc<Self>) {
        info!("Starting MM2 health monitor");
        
        let mut interval = interval(Duration::from_secs(30));
        
        loop {
            interval.tick().await;
            
            let state = *self.state.read().await;
            
            // Only monitor if running
            if state != Mm2State::Running {
                continue;
            }

            // Check health
            let health = self.health().await;
            
            if !health.rpc_reachable {
                warn!("MM2 RPC unreachable, marking as unhealthy");
                *self.state.write().await = Mm2State::Unhealthy;
                
                // Attempt restart
                if let Err(e) = self.restart().await {
                    error!("Failed to restart MM2: {}", e);
                    *self.state.write().await = Mm2State::Failed;
                }
            } else {
                debug!("MM2 health check passed (uptime: {}s, coins: {})",
                    health.uptime_secs, health.enabled_coins);
                
                // Restore to running if was unhealthy
                if state == Mm2State::Unhealthy {
                    info!("MM2 recovered, marking as running");
                    *self.state.write().await = Mm2State::Running;
                }
            }
        }
    }

    /// Get MM2 client for API calls
    pub fn client(&self) -> Arc<Mm2Client> {
        Arc::clone(&self.client)
    }

    // Private helper methods

    async fn write_config_file(&self) -> Result<PathBuf> {
        // TODO: Write MM2 config JSON to file
        // For now, return placeholder
        Ok(PathBuf::from("mm2.json"))
    }

    async fn wait_for_ready(&self) -> Result<()> {
        info!("Waiting for MM2 to be ready...");
        
        let max_attempts = 30; // 30 seconds
        let mut attempts = 0;

        while attempts < max_attempts {
            tokio::time::sleep(Duration::from_secs(1)).await;
            attempts += 1;

            if self.check_rpc_reachable().await {
                info!("MM2 RPC is reachable after {} seconds", attempts);
                return Ok(());
            }
        }

        Err(Error::Manager(
            "MM2 did not become ready within 30 seconds".to_string()
        ))
    }

    async fn check_rpc_reachable(&self) -> bool {
        // Try to call version endpoint
        self.client.version().await.is_ok()
    }

    async fn get_enabled_coins_count(&self) -> usize {
        match self.client.get_enabled_coins().await {
            Ok(coins) => coins.len(),
            Err(_) => 0,
        }
    }
}

impl Drop for Mm2Manager {
    fn drop(&mut self) {
        // Ensure MM2 is stopped on drop
        if let Some(mut child) = self.process.blocking_write().take() {
            let _ = child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_manager_lifecycle() {
        let config = Mm2Config {
            rpc_port: 7783,
            userpass: "test".to_string(),
            ..Default::default()
        };

        let manager = Mm2Manager::new(
            PathBuf::from("/tmp/mm2"),
            config,
        );

        // Initial state should be stopped
        assert_eq!(manager.state().await, Mm2State::Stopped);

        // Health should reflect stopped state
        let health = manager.health().await;
        assert_eq!(health.state, Mm2State::Stopped);
        assert_eq!(health.uptime_secs, 0);
    }

    #[tokio::test]
    async fn test_cannot_start_twice() {
        let config = Mm2Config {
            rpc_port: 7783,
            userpass: "test".to_string(),
            ..Default::default()
        };

        let manager = Mm2Manager::new(
            PathBuf::from("/tmp/mm2"),
            config,
        );

        // Mock "running" state
        *manager.state.write().await = Mm2State::Running;

        // Should error
        let result = manager.start().await;
        assert!(result.is_err());
    }
}
