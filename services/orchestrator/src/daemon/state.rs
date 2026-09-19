//! Estado compartilhado do daemon no processo do orchestrator (D1).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::client::DaemonClient;
use super::launcher::DaemonLauncher;

/// Estado compartilhado do daemon no processo do orchestrator.
/// Cada orchestrator = 1 nó (padrão da casa).
///
/// Mantém referências ao client e launcher para que preempção,
/// housekeeping e o path de generate possam operar sem criar
/// instâncias novas.
pub struct DaemonState {
    pub running: std::sync::Mutex<bool>,
    pub url: std::sync::Mutex<Option<String>>,
    pub last_used: std::sync::Mutex<Instant>,
    pub loaded_spec: std::sync::Mutex<Option<String>>,
    pub image: String,
    pub container_name: String,
    pub port: u16,
    pub idle_ttl: Duration,
    pub client: std::sync::RwLock<Arc<dyn DaemonClient>>,
    pub launcher: Arc<dyn DaemonLauncher>,
}

impl DaemonState {
    pub fn new(
        image: &str,
        port: u16,
        idle_ttl_secs: u64,
        client: Arc<dyn DaemonClient>,
        launcher: Arc<dyn DaemonLauncher>,
    ) -> Self {
        Self {
            running: std::sync::Mutex::new(false),
            url: std::sync::Mutex::new(None),
            last_used: std::sync::Mutex::new(Instant::now()),
            loaded_spec: std::sync::Mutex::new(None),
            image: image.to_string(),
            container_name: "diffusion-daemon".to_string(),
            port,
            idle_ttl: Duration::from_secs(idle_ttl_secs),
            client: std::sync::RwLock::new(client),
            launcher,
        }
    }

    /// Substitui o client interno (usado após launcher.start() retornar a URL real).
    pub fn set_client(&self, client: Arc<dyn DaemonClient>) {
        *self.client.write().unwrap() = client;
    }

    pub fn touch(&self) {
        *self.last_used.lock().unwrap() = Instant::now();
    }

    pub fn is_running(&self) -> bool {
        *self.running.lock().unwrap()
    }

    pub fn get_url(&self) -> Option<String> {
        self.url.lock().unwrap().clone()
    }

    pub fn set_running(&self, running: bool, url: Option<String>) {
        *self.running.lock().unwrap() = running;
        *self.url.lock().unwrap() = url;
        if running {
            self.touch();
        }
    }

    pub fn get_loaded_spec(&self) -> Option<String> {
        self.loaded_spec.lock().unwrap().clone()
    }

    pub fn set_loaded_spec(&self, spec: String) {
        *self.loaded_spec.lock().unwrap() = Some(spec);
    }

    pub fn is_idle(&self, busy: bool) -> bool {
        if busy {
            return false;
        }
        let last_used = *self.last_used.lock().unwrap();
        last_used.elapsed() > self.idle_ttl / 2
    }
}
