import { Channel } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import {
  forwardApi,
  type ForwardFrame,
  type ForwardOpenRequest,
  type ForwardView,
} from "./portForwardApi";
import { errorOf, type Server } from "./types";

export function usePortForwards() {
  const [rules, setRules] = useState<ForwardView[]>([]);
  const [servers, setServers] = useState<Server[]>([]);
  const [generation, setGeneration] = useState<number | null>(null);
  const [error, setError] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [openRequest, setOpenRequest] = useState<ForwardOpenRequest | null>(
    null,
  );
  const lastRequest = useRef("");
  useEffect(() => {
    let disposed = false,
      subscription = "",
      currentGeneration = 0,
      lastRevision = 0;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let watchdog: ReturnType<typeof setInterval> | undefined;
    let lastFrame = Date.now(),
      connecting = false,
      epoch = 0;
    async function connect() {
      if (disposed || connecting) return;
      connecting = true;
      const attempt = ++epoch;
      try {
        if (subscription)
          await forwardApi
            .unsubscribe(currentGeneration, subscription)
            .catch(() => {});
        const boot = await forwardApi.bootstrap();
        if (disposed) return;
        currentGeneration = boot.generation;
        setGeneration(boot.generation);
        setServers(boot.servers);
        lastRevision = 0;
        const output = new Channel<ForwardFrame>();
        output.onmessage = (frame) => {
          if (disposed || attempt !== epoch || frame.revision <= lastRevision)
            return;
          lastRevision = frame.revision;
          lastFrame = Date.now();
          setRules(frame.rules);
          setServers(frame.servers);
          setError(frame.error?.message || "");
          setLoaded(true);
          if (
            frame.openRequest &&
            frame.openRequest.id !== lastRequest.current
          ) {
            lastRequest.current = frame.openRequest.id;
            setOpenRequest(frame.openRequest);
          }
          void forwardApi
            .ack(currentGeneration, frame.subscriptionId, frame.revision)
            .catch((e) => {
              if (!disposed) setError(errorOf(e).message);
            });
        };
        subscription = await forwardApi.subscribe(boot.generation, output);
        lastFrame = Date.now();
        if (disposed)
          await forwardApi
            .unsubscribe(boot.generation, subscription)
            .catch(() => {});
      } catch (e) {
        if (!disposed) {
          setError(errorOf(e).message);
          retry = setTimeout(() => void connect(), 2000);
        }
      } finally {
        connecting = false;
      }
    }
    void connect();
    watchdog = setInterval(() => {
      if (Date.now() - lastFrame > 5000 && !connecting) void connect();
    }, 2000);
    return () => {
      disposed = true;
      clearTimeout(retry);
      clearInterval(watchdog);
      if (subscription)
        void forwardApi
          .unsubscribe(currentGeneration, subscription)
          .catch(() => {});
    };
  }, []);
  return {
    rules,
    servers,
    generation,
    error,
    loaded,
    openRequest,
  };
}
