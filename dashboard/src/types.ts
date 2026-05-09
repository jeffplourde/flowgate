export interface ScoreEvent {
  type: "score";
  instance: string;
  score: number;
  threshold: number;
  state: string;
}

export interface MetricsEvent {
  type: "metrics";
  instance: string;
  data: Record<string, number>;
}

export type FlowgateEvent = ScoreEvent | MetricsEvent;

export interface TimeSeriesPoint {
  time: number;
  value: number;
}

export interface InstanceState {
  ingestionRate: TimeSeriesPoint[];
  emissionRate: TimeSeriesPoint[];
  threshold: TimeSeriesPoint[];
  bufferSize: TimeSeriesPoint[];
  avgScore: number;
  avgLatency: number;
  received: number;
  emitted: number;
  rejected: number;
  evicted: number;
  controllerState: string;
  pidError: number;
  pidP: number;
  pidI: number;
  pidD: number;
  targetRate: number;
  algorithm: string;
  scoreHistory: number[];
}

export interface ProducerState {
  published: number;
  errors: number;
  batches: number;
  activeClients: number;
  backpressure: boolean;
}

export const EMPTY_PRODUCER: ProducerState = {
  published: 0,
  errors: 0,
  batches: 0,
  activeClients: 0,
  backpressure: false,
};

export const EMPTY_INSTANCE: InstanceState = {
  ingestionRate: [],
  emissionRate: [],
  threshold: [],
  bufferSize: [],
  avgScore: 0,
  avgLatency: 0,
  received: 0,
  emitted: 0,
  rejected: 0,
  evicted: 0,
  controllerState: "unknown",
  pidError: 0,
  pidP: 0,
  pidI: 0,
  pidD: 0,
  targetRate: 10,
  algorithm: "pid",
  scoreHistory: [],
};
