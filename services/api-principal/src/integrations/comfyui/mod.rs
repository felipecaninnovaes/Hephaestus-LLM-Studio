//! Integração ComfyUI: destinos cadastrados + envio de LoRA em partes ao
//! custom node `integrations/comfyui-hephaestus` (contrato §1/§2).

pub mod client;
pub mod crypto;
pub mod handlers;
pub mod repository;
pub mod runner;
pub mod safetensors;
pub mod source;
pub mod types;
