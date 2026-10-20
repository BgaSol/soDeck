import { useEffect, useSyncExternalStore } from "react";
import { desktop } from "./api";
import { updateApi, type UpdateSnapshot } from "./updateApi";
let snapshot: UpdateSnapshot | null = null;
const subscribers = new Set<() => void>();
let started = false;
export function receiveUpdate(next: UpdateSnapshot) {
  if (!next || (snapshot && next.revision < snapshot.revision)) return;
  snapshot = next;
  subscribers.forEach((fn) => fn());
}
export function useAppUpdate() {
  useEffect(() => {
    if (!desktop || started) return;
    started = true;
    // Subscribe before reading: revisions prevent a slow read replacing newer events.
    void updateApi
      .subscribe(receiveUpdate)
      .then(() => updateApi.snapshot().then(receiveUpdate))
      .catch(() => {
        started = false;
      });
  }, []);
  return useSyncExternalStore(
    (fn) => {
      subscribers.add(fn);
      return () => {
        subscribers.delete(fn);
      };
    },
    () => snapshot,
  );
}
