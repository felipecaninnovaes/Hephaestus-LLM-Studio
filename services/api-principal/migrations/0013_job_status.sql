-- 0013_job_status.sql — AC-006-A (ADR-0024 D3): snapshot do último status no job.
ALTER TABLE jobs ADD COLUMN phase TEXT;
ALTER TABLE jobs ADD COLUMN message TEXT;
