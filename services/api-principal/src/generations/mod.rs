//! Domínio generations — galeria persistente de imagens geradas (ADR-0023 D5).
//!
//! Stubs nesta fatia (G.1) retornam 503 `queue_unavailable` até o manager
//! expor as rotas internas e o BFF implementar o proxy real (G.6).

pub mod handlers;
