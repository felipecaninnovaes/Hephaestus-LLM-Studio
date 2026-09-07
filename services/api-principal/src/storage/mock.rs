//! `MockStorage` para testes SEM REDE (D8).
//!
//! `S3Storage` de verdade só chega na 3b.4 — este mock sustenta o boot
//! (`STORAGE_BACKEND=mock`) e os testes de cascata da ADR.

use std::collections::HashMap;
use std::path::Path;

use tokio::sync::RwLock;

use super::port::{StorageConfig, StorageError, StoragePort};

/// Storage em memória com log de operações (`DELETE_PREFIX <prefix>` é
/// registrado — usado pelo teste de cascata da tabela "Testes" da ADR).
pub struct MockStorage {
    bucket: String,
    url_ttl_secs: u64,
    objects: RwLock<HashMap<String, Vec<u8>>>,
    ops: RwLock<Vec<String>>,
    failing: std::sync::atomic::AtomicBool,
    /// Falha injetada após N PUTs bem-sucedidos (teste de falha no meio do
    /// ingest do import, 3e.2): `usize::MAX` = nunca. O PUT que estoura
    /// retorna `Unavailable("injected")` sem gravar op.
    fail_after_puts: std::sync::atomic::AtomicUsize,
    puts_done: std::sync::atomic::AtomicUsize,
}

impl MockStorage {
    /// Cria um mock vazio (bucket `heph-test`, TTL 60s).
    pub fn new() -> Self {
        Self::with_config("heph-test".to_string(), 60)
    }

    fn with_config(bucket: String, url_ttl_secs: u64) -> Self {
        Self {
            bucket,
            url_ttl_secs,
            objects: RwLock::new(HashMap::new()),
            ops: RwLock::new(Vec::new()),
            failing: std::sync::atomic::AtomicBool::new(false),
            fail_after_puts: std::sync::atomic::AtomicUsize::new(usize::MAX),
            puts_done: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Simula bucket morto — testes dos ramos 503 sem rede: todos os
    /// métodos da porta retornam `Err(StorageError::Unavailable("injected"))`
    /// e nenhuma op é gravada.
    pub fn failing() -> Self {
        let m = Self::with_config("heph-test".to_string(), 60);
        m.failing.store(true, std::sync::atomic::Ordering::SeqCst);
        m
    }

    fn is_failing(&self) -> bool {
        self.failing.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Os próximos `n` PUTs passam; do `n+1`-ésimo em diante, `put` retorna
    /// `Unavailable("injected")`. `put_bytes` (semeadura de fixture) não
    /// conta nem falha — só o `put` da porta.
    pub fn fail_after_puts(&self, n: usize) {
        self.fail_after_puts
            .store(n, std::sync::atomic::Ordering::SeqCst);
    }

    /// `key→bytes` ordenado por key (para asserções de teste).
    pub fn snapshot(&self) -> Vec<(String, usize)> {
        // Sync (chamável fora de `.await`): `try_read` nunca cruza await nem
        // mantém lock entre awaits externos; operações são curtas.
        let objects = self.objects.try_read().expect("mock objects lock");
        let mut out: Vec<(String, usize)> =
            objects.iter().map(|(k, v)| (k.clone(), v.len())).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Log de operações (`PUT <key>` / `GET <key>` / `DELETE <key>` /
    /// `COPY <from> <to>` / `DELETE_PREFIX <prefix>`).
    pub fn ops(&self) -> Vec<String> {
        self.ops.try_read().expect("mock ops lock").clone()
    }

    /// Helper p/ semear fixtures sem tempfile.
    pub async fn put_bytes(&self, key: &str, data: Vec<u8>) {
        {
            let mut objects = self.objects.write().await;
            objects.insert(key.to_string(), data);
        }
        {
            let mut ops = self.ops.write().await;
            ops.push(format!("PUT {key}"));
        }
    }

    /// Config espelho do mock (bucket/TTL usados no `presign_get`).
    pub fn test_config() -> StorageConfig {
        StorageConfig {
            bucket: "heph-test".into(),
            public_endpoint: None,
            url_ttl_secs: 60,
        }
    }
}

impl Default for MockStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl StoragePort for MockStorage {
    async fn put(&self, key: &str, path: &Path) -> Result<(), StorageError> {
        if self.is_failing() {
            return Err(StorageError::Unavailable("injected".to_string()));
        }
        // `fail_after_puts(n)` deixa passar os PUTs #0..#n-1 e injeta do #n
        // em diante (default `MAX` = nunca). `put_bytes` não conta.
        let done = self
            .puts_done
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if done
            >= self
                .fail_after_puts
                .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(StorageError::Unavailable("injected".to_string()));
        }
        let data = tokio::fs::read(path)
            .await
            .map_err(|_| StorageError::Unavailable("storage unavailable".to_string()))?;
        {
            let mut objects = self.objects.write().await;
            objects.insert(key.to_string(), data);
        }
        {
            let mut ops = self.ops.write().await;
            ops.push(format!("PUT {key}"));
        }
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, StorageError> {
        if self.is_failing() {
            return Err(StorageError::Unavailable("injected".to_string()));
        }
        let data = {
            let objects = self.objects.read().await;
            objects.get(key).cloned()
        };
        {
            let mut ops = self.ops.write().await;
            ops.push(format!("GET {key}"));
        }
        data.ok_or(StorageError::NotFound)
    }

    async fn get_to_file(&self, key: &str, path: &Path) -> Result<(), StorageError> {
        if self.is_failing() {
            return Err(StorageError::Unavailable("injected".to_string()));
        }
        let data = {
            let objects = self.objects.read().await;
            objects.get(key).cloned()
        };
        let data = match data {
            Some(d) => d,
            None => return Err(StorageError::NotFound),
        };
        tokio::fs::write(path, &data)
            .await
            .map_err(|_| StorageError::Unavailable("storage unavailable".to_string()))?;
        {
            let mut ops = self.ops.write().await;
            ops.push(format!("GET_TO_FILE {key}"));
        }
        Ok(())
    }

    async fn presign_get(&self, key: &str) -> Result<String, StorageError> {
        if self.is_failing() {
            return Err(StorageError::Unavailable("injected".to_string()));
        }
        Ok(format!(
            "mock://{}/{key}?ttl={}",
            self.bucket, self.url_ttl_secs
        ))
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        if self.is_failing() {
            return Err(StorageError::Unavailable("injected".to_string()));
        }
        {
            let mut objects = self.objects.write().await;
            objects.remove(key);
        }
        {
            let mut ops = self.ops.write().await;
            ops.push(format!("DELETE {key}"));
        }
        Ok(())
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<u32, StorageError> {
        if self.is_failing() {
            return Err(StorageError::Unavailable("injected".to_string()));
        }
        {
            let mut ops = self.ops.write().await;
            ops.push(format!("DELETE_PREFIX {prefix}"));
        }
        let removed: u32;
        {
            let mut objects = self.objects.write().await;
            let keys: Vec<String> = objects
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect();
            removed = keys.len() as u32;
            for k in keys {
                objects.remove(&k);
            }
        }
        Ok(removed)
    }

    async fn copy_object(&self, from_key: &str, to_key: &str) -> Result<(), StorageError> {
        if self.is_failing() {
            return Err(StorageError::Unavailable("injected".to_string()));
        }
        let data = {
            let objects = self.objects.read().await;
            objects.get(from_key).cloned()
        };
        let data = match data {
            Some(d) => d,
            None => return Err(StorageError::NotFound),
        };
        {
            let mut objects = self.objects.write().await;
            objects.insert(to_key.to_string(), data);
        }
        {
            let mut ops = self.ops.write().await;
            ops.push(format!("COPY {from_key} {to_key}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip_via_put_bytes() {
        let m = MockStorage::new();
        m.put_bytes("k/a.bin", vec![1, 2, 3]).await;
        let got = m.get("k/a.bin").await.expect("get");
        assert_eq!(got, vec![1, 2, 3]);
        assert_eq!(m.snapshot(), vec![("k/a.bin".to_string(), 3)]);
        assert!(m.ops().contains(&"PUT k/a.bin".to_string()));
        assert!(m.ops().contains(&"GET k/a.bin".to_string()));
    }

    #[tokio::test]
    async fn put_reads_real_file_from_disk() {
        let m = MockStorage::new();
        let path = std::env::temp_dir().join(format!(
            "heph-mock-put-{}.bin",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::write(&path, [9u8, 8, 7]).expect("seed tempfile");
        m.put("k/f.bin", &path).await.expect("put");
        std::fs::remove_file(&path).ok();
        assert_eq!(m.get("k/f.bin").await.expect("get"), vec![9, 8, 7]);
    }

    #[tokio::test]
    async fn delete_prefix_removes_only_scoped_keys() {
        let m = MockStorage::new();
        m.put_bytes("ds/1/a", vec![1]).await;
        m.put_bytes("ds/1/b", vec![2]).await;
        m.put_bytes("ds/1/c", vec![3]).await;
        m.put_bytes("ds/2/z", vec![4]).await;
        let n = m.delete_prefix("ds/1/").await.expect("delete_prefix");
        assert_eq!(n, 3);
        assert!(m.ops().contains(&"DELETE_PREFIX ds/1/".to_string()));
        assert_eq!(m.get("ds/2/z").await.expect("fora do prefixo"), vec![4]);
        assert!(matches!(m.get("ds/1/a").await, Err(StorageError::NotFound)));
    }

    #[tokio::test]
    async fn get_to_file_observavel() {
        let m = MockStorage::new();
        m.put_bytes("k/a.bin", vec![4, 5, 6]).await;
        let path = std::env::temp_dir().join(format!(
            "heph-mock-get-to-file-{}.bin",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        m.get_to_file("k/a.bin", &path).await.expect("get_to_file");
        assert_eq!(std::fs::read(&path).expect("read"), vec![4, 5, 6]);
        std::fs::remove_file(&path).ok();
        assert!(m.ops().contains(&"GET_TO_FILE k/a.bin".to_string()));
        assert!(matches!(
            m.get_to_file(
                "ausente",
                &std::env::temp_dir().join("heph-mock-get-to-file-ausente.bin")
            )
            .await,
            Err(StorageError::NotFound)
        ));
    }

    #[tokio::test]
    async fn get_missing_is_not_found() {
        let m = MockStorage::new();
        assert!(matches!(m.get("nope").await, Err(StorageError::NotFound)));
    }

    #[tokio::test]
    async fn presign_get_is_deterministic() {
        let m = MockStorage::new();
        let url = m.presign_get("k/a.bin").await.expect("presign");
        assert_eq!(url, "mock://heph-test/k/a.bin?ttl=60");
    }
}
