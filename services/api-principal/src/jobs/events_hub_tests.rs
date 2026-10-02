//! Testes de `events_hub` (fatia 1b) — fan-out, coalescência e limpeza de
//! assinantes, com `MockManager` (sem Postgres).

use super::*;
use crate::jobs::manager_client::MockManager;
use heph_contracts::telemetry::{MetricPointWithKey, MetricPointsResponse};
use std::sync::Arc as StdArc;

fn mock_with_one_point(seq: i64) -> MockManager {
    let mut mock = MockManager::default();
    mock.metric_points_result = Some(MetricPointsResponse {
        items: vec![MetricPointWithKey {
            seq,
            epoch: Some(1),
            step: 0,
            key: "loss".into(),
            value: 0.5,
            ts: "2024-01-01T00:00:00Z".into(),
        }],
        max_seq: seq,
        downsampled: false,
    });
    mock
}

/// 3 assinantes do mesmo job recebem o MESMO evento `metrics` de um
/// único `process_job` (UMA busca ao manager, fan-out pronto).
#[tokio::test]
async fn fan_out_broadcasts_same_event_to_all_subscribers() {
    let hub = JobEventsHub::new();
    let job_id = "11111111-1111-1111-1111-111111111111";
    let mut rx1 = hub.subscribe(job_id).await;
    let mut rx2 = hub.subscribe(job_id).await;
    let mut rx3 = hub.subscribe(job_id).await;

    let mock = mock_with_one_point(7);
    let manager: StdArc<dyn ManagerPort> = StdArc::new(mock);

    process_job(&hub, manager.as_ref(), job_id, true, false).await;

    let e1 = rx1.recv().await.expect("rx1 recebe");
    let e2 = rx2.recv().await.expect("rx2 recebe");
    let e3 = rx3.recv().await.expect("rx3 recebe");
    assert_eq!(e1.event, "metrics");
    assert_eq!(e1.id.as_deref(), Some("7"));
    assert_eq!(e1.data, e2.data);
    assert_eq!(e2.data, e3.data);
}

/// Rajada de 10 `schedule()` do mesmo job dentro da janela de debounce
/// (100ms) colapsa numa ÚNICA busca ao manager — não 10.
#[tokio::test]
async fn burst_of_notices_coalesces_into_one_fetch() {
    let hub = JobEventsHub::new();
    let job_id = "22222222-2222-2222-2222-222222222222".to_string();
    let mut rx = hub.subscribe(&job_id).await;

    let mock_concrete = StdArc::new(mock_with_one_point(1));
    let manager: StdArc<dyn ManagerPort> = mock_concrete.clone();

    for _ in 0..10 {
        schedule(
            StdArc::clone(&hub),
            StdArc::clone(&manager),
            job_id.clone(),
            true,
            false,
        )
        .await;
    }
    // Aguarda além da janela de debounce (100ms) pro fetch coalescido rodar.
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(
        *mock_concrete.metric_points_calls.lock().unwrap(),
        1,
        "10 notices na mesma janela devem gerar 1 única busca ao manager"
    );
    // E o assinante recebeu exatamente 1 evento (sem duplicata por notice).
    let ev = rx.recv().await.expect("evento coalescido");
    assert_eq!(ev.event, "metrics");
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

/// Último assinante saindo remove o `Sender` do mapa interno.
#[tokio::test]
async fn unsubscribe_removes_channel_when_empty() {
    let hub = JobEventsHub::new();
    let job_id = "33333333-3333-3333-3333-333333333333";
    let rx1 = hub.subscribe(job_id).await;
    let rx2 = hub.subscribe(job_id).await;
    assert!(hub.has_subscribers(job_id).await);

    drop(rx1);
    hub.unsubscribe_if_empty(job_id).await;
    assert!(
        hub.channels.lock().await.contains_key(job_id),
        "ainda há 1 assinante (rx2) — não deve remover"
    );

    drop(rx2);
    hub.unsubscribe_if_empty(job_id).await;
    assert!(
        !hub.channels.lock().await.contains_key(job_id),
        "0 assinantes — deve remover a entrada"
    );
}
/// Quando o listener reconecta (ou detecta desconexão via try_recv -> Ok(None)),
/// refetch_subscribed_jobs refaz o delta (afterSeq = last_seq) e o status para todos
/// os jobs com assinantes ativos — permitindo que reports emitidos durante a queda
/// cheguem aos assinantes sem esperar por um novo notice.
#[tokio::test]
async fn reconnect_refetches_delta_and_status_for_subscribers() {
    let hub = JobEventsHub::new();
    let job_id = "44444444-4444-4444-4444-444444444444";
    let mut rx = hub.subscribe(job_id).await;

    // 1. Hub inicialmente processa o seq 5
    let mut mock = MockManager::default();
    mock.metric_points_result = Some(MetricPointsResponse {
        items: vec![MetricPointWithKey {
            seq: 5,
            key: "loss".to_string(),
            step: 50,
            epoch: Some(1),
            value: 0.5,
            ts: "2026-01-01T00:00:00Z".to_string(),
        }],
        max_seq: 5,
        downsampled: false,
    });
    let manager: StdArc<dyn ManagerPort> = StdArc::new(mock);
    process_job(&hub, manager.as_ref(), job_id, true, false).await;

    let ev1 = rx.recv().await.expect("recebe seq 5");
    assert_eq!(ev1.id.as_deref(), Some("5"));
    assert_eq!(hub.known_seq(job_id).await, Some(5));

    // 2. Durante a queda, o manager recebe novos pontos (seq 6 e 7)
    let mut mock2 = MockManager::default();
    mock2.metric_points_result = Some(MetricPointsResponse {
        items: vec![
            MetricPointWithKey {
                seq: 6,
                key: "loss".to_string(),
                step: 60,
                epoch: Some(1),
                value: 0.4,
                ts: "2026-01-01T00:00:01Z".to_string(),
            },
            MetricPointWithKey {
                seq: 7,
                key: "loss".to_string(),
                step: 70,
                epoch: Some(1),
                value: 0.3,
                ts: "2026-01-01T00:00:02Z".to_string(),
            },
        ],
        max_seq: 7,
        downsampled: false,
    });
    let manager2: StdArc<dyn ManagerPort> = StdArc::new(mock2);

    // 3. Simula reconexão chamando refetch_subscribed_jobs
    hub.refetch_subscribed_jobs(manager2.as_ref()).await;

    // O assinante deve receber o delta acumulado durante a queda
    let ev2 = rx.recv().await.expect("recebe delta da reconexão");
    assert_eq!(ev2.event, "metrics");
    assert_eq!(ev2.id.as_deref(), Some("7"));
    assert_eq!(hub.known_seq(job_id).await, Some(7));
}
