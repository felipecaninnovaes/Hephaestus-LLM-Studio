//! Domínio jobs (principal como BFF do manager).
//!
//! O principal NÃO lê as tabelas jobs/orchestrators/job_artifacts diretamente —
//! dono é o manager (ADR-0007 D1/D3/D8). Este módulo contém o client HTTP do
//! manager e os handlers de leitura que compõem o BFF.

pub mod handlers;
pub mod manager_client;
