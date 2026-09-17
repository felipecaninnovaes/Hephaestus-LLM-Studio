//! Domínio generations — galeria persistente de imagens geradas (ADR-0023 D5).
//!
//! BFF do manager: rotas públicas com presigned URLs condicionais,
//! proxy de imagem via StoragePort, delete em lote e export ZIP.

pub mod handlers;
pub mod inputs;
