export interface Telemetry {
  measured: boolean;
  cpu: number | null;
  ram: number | null;
  ramTotal: number | null;
  vramUsed: number | null;
  vramTotal: number | null;
  gpus: string[];
  jobsActive: number;
}
