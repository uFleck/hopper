import type { EventMap, EventName } from "./types";

type Handler<K extends EventName> = (payload: EventMap[K]) => void;

/** A tiny in-memory event bus: stands in for Tauri events in a plain browser (the mock emits on it) and in tests. */
class Bus {
  private handlers = new Map<EventName, Set<Handler<EventName>>>();

  on<K extends EventName>(event: K, handler: Handler<K>): () => void {
    let set = this.handlers.get(event);
    if (!set) this.handlers.set(event, (set = new Set()));
    set.add(handler as Handler<EventName>);
    return () => void set.delete(handler as Handler<EventName>);
  }

  emit<K extends EventName>(event: K, ...payload: EventMap[K] extends undefined ? [] : [EventMap[K]]): void {
    for (const h of [...(this.handlers.get(event) ?? [])]) h(payload[0] as EventMap[K]);
  }
}

export const bus = new Bus();
