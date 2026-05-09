import { useEffect, useRef, useState, useCallback } from "react";
import type {
  FlowgateEvent,
  InstanceState,
  ProducerState,
  TimeSeriesPoint,
} from "../types";
import { EMPTY_INSTANCE, EMPTY_PRODUCER } from "../types";

const MAX_SERIES_POINTS = 120;
const MAX_SCORE_HISTORY = 500;

const STATE_NAMES: Record<number, string> = {
  0: "cold_start",
  1: "warmup",
  2: "pid",
  3: "fixed",
};

function pushPoint(
  arr: TimeSeriesPoint[],
  value: number
): TimeSeriesPoint[] {
  const next = [...arr, { time: Date.now(), value }];
  return next.length > MAX_SERIES_POINTS
    ? next.slice(next.length - MAX_SERIES_POINTS)
    : next;
}

export function useFlowgateSocket() {
  const [instanceA, setInstanceA] = useState<InstanceState>({
    ...EMPTY_INSTANCE,
  });
  const [instanceB, setInstanceB] = useState<InstanceState>({
    ...EMPTY_INSTANCE,
  });
  const [producer, setProducer] = useState<ProducerState>({
    ...EMPTY_PRODUCER,
  });
  const [connected, setConnected] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsUrl = `${protocol}//${window.location.host}/ws`;
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => setConnected(true);
    ws.onclose = () => {
      setConnected(false);
      setTimeout(() => {
        wsRef.current = new WebSocket(wsUrl);
      }, 2000);
    };

    ws.onmessage = (evt) => {
      const event: FlowgateEvent = JSON.parse(evt.data);

      if (event.instance === "producer" && event.type === "metrics") {
        const d = event.data;
        setProducer({
          published: d.producer_messages_published_total ?? 0,
          errors: d.producer_publish_errors_total ?? 0,
          batches: d.producer_batches_sent_total ?? 0,
          activeClients: d.producer_active_clients ?? 0,
          backpressure: (d.producer_backpressure ?? 0) > 0.5,
        });
        return;
      }

      const setter = event.instance === "a" ? setInstanceA : setInstanceB;

      if (event.type === "score") {
        setter((prev) => ({
          ...prev,
          scoreHistory: [
            ...prev.scoreHistory.slice(-(MAX_SCORE_HISTORY - 1)),
            event.score,
          ],
        }));
      } else if (event.type === "metrics") {
        const d = event.data;
        setter((prev) => ({
          ...prev,
          ingestionRate: pushPoint(
            prev.ingestionRate,
            d.flowgate_ingestion_rate ?? 0
          ),
          emissionRate: pushPoint(
            prev.emissionRate,
            d.flowgate_actual_rate ?? 0
          ),
          threshold: pushPoint(
            prev.threshold,
            d.flowgate_current_threshold ?? 0
          ),
          bufferSize: pushPoint(prev.bufferSize, d.flowgate_buffer_size ?? 0),
          avgScore: d.flowgate_avg_emitted_score ?? prev.avgScore,
          avgLatency: d.flowgate_avg_latency_ms ?? prev.avgLatency,
          received: d.flowgate_messages_received_total ?? prev.received,
          emitted: d.flowgate_messages_emitted_total ?? prev.emitted,
          rejected: d.flowgate_messages_rejected_total ?? prev.rejected,
          evicted: d.flowgate_buffer_evicted_total ?? prev.evicted,
          controllerState:
            STATE_NAMES[d.flowgate_controller_state ?? -1] ??
            prev.controllerState,
          pidError: d.flowgate_pid_error ?? prev.pidError,
          pidP: d.flowgate_pid_p_term ?? prev.pidP,
          pidI: d.flowgate_pid_i_term ?? prev.pidI,
          pidD: d.flowgate_pid_d_term ?? prev.pidD,
          targetRate: d.flowgate_target_rate ?? prev.targetRate,
        }));
      }
    };

    return () => ws.close();
  }, []);

  const updateConfig = useCallback(
    async (instance: string, key: string, value: string) => {
      await fetch(`/api/config/${instance}/${key}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ value }),
      });
    },
    []
  );

  return { instanceA, instanceB, producer, connected, updateConfig };
}
