import { useEffect, useSyncExternalStore } from "react";
import { desktop } from "./api";
import { updateApi, type UpdateSnapshot } from "./updateApi";

let snapshot: UpdateSnapshot | null = null;
const subscribers = new Set<() => void>();
let started = false;
let generation = 0;

export function receiveUpdate(next: UpdateSnapshot) {
  if (!next || (snapshot && next.revision < snapshot.revision)) return;
  snapshot = next;
  subscribers.forEach((fn) => fn());
}

function connect() {
  if (started || !desktop) return;
  started = true;
  const attempt = ++generation;
  void updateApi
    .subscribe(receiveUpdate)
    .then((off) => {
      if (attempt !== generation) {
        off();
        return;
      }
      return updateApi
        .snapshot()
        .then(receiveUpdate)
        .catch(() => {
          off();
          started = false;
          setTimeout(connect, 3000);
        });
    })
    .catch(() => {
      if (attempt !== generation) return;
      started = false;
      setTimeout(connect, 3000);
    });
}

export function useAppUpdate() {
  useEffect(() => {
    connect();
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
