import React, { useEffect, useRef, useState } from 'react';

type BlockCard = {
  height: number;
  txs?: number;
  timestamp?: string;
  miner?: string;
};

type Props = {
  apiUrl: string;
};

export default function BlockScroller({ apiUrl }: Props) {
  const [items, setItems] = useState<BlockCard[]>([]);
  const queueRef = useRef<BlockCard[]>([]);
  const rowRef = useRef<HTMLDivElement | null>(null);
  const animRef = useRef<number | null>(null);
  const pausedRef = useRef<boolean>(false);
  const offsetRef = useRef<number>(0);

  useEffect(() => {
    let ws: WebSocket | null = null;
    try {
      const url = (process.env.NEXT_PUBLIC_WS_URL as string) || 'ws://localhost:8081';
      ws = new WebSocket(url);
      ws.onopen = () => {
        const sub = JSON.stringify({ method: 'subscribe', params: { topic: 'block', filter: {} }, request_id: 'ui_block_scroller' });
        ws?.send(sub);
      };
      ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(ev.data);
          const topic = msg.topic || msg?.result?.topic;
          const data = msg.data || msg?.result?.data;
          if (topic === 'block' && data) {
            const height = data.height ?? data.block_height ?? data.number;
            const txs = data.transaction_count ?? data.txs?.length ?? 0;
            const timestamp = data.timestamp ?? data.time ?? null;
            const card: BlockCard = { height, txs, timestamp: timestamp || undefined };
            // enqueue and kick animation loop
            queueRef.current.push(card);
          }
        } catch {}
      };
    } catch {}
    return () => { try { ws?.close(); } catch {} };
  }, [apiUrl]);

  useEffect(() => {
    // Seed with recent blocks on mount
    (async () => {
      try {
        const res = await fetch(`${apiUrl}/api/blocks?limit=10&offset=0`);
        const json = await res.json();
        const blocks = Array.isArray(json?.blocks) ? json.blocks : [];
        const seeded = blocks.map((b: any) => ({ height: b.height, txs: b.transaction_count, timestamp: b.timestamp }));
        setItems(seeded);
        // play initial sweep once by scheduling them in the queue; we won't loop endlessly
        queueRef.current.push(...seeded);
      } catch {}
    })();
  }, [apiUrl]);

  const playNextBatch = () => {
    if (!rowRef.current) return;
    // take next item(s) from queue
    const next = queueRef.current.shift();
    if (!next) return; // nothing to animate
    // prepend the card and animate a left shift equal to card width + gap
    const cardWidth = 212; // min-width 200 + gap
    setItems((prev) => [next, ...prev].slice(0, 20));
    const start = offsetRef.current;
    const end = start - cardWidth;
    const el = rowRef.current;
    const startTime = performance.now();
    const duration = 800; // ms per card
    const tick = (t: number) => {
      const p = Math.min(1, (t - startTime) / duration);
      const val = start + (end - start) * p;
      offsetRef.current = val;
      el.style.transform = `translateX(${val}px)`;
      if (p < 1) {
        animRef.current = requestAnimationFrame(tick);
      } else {
        // reset offset when a full card has moved
        offsetRef.current = 0;
        el.style.transform = 'translateX(0px)';
        // recursively play the next batch if any
        if (queueRef.current.length > 0) playNextBatch();
      }
    };
    animRef.current = requestAnimationFrame(tick);
  };

  useEffect(() => {
    // batch animation loop: only play when queue has items and not paused
    const loop = () => {
      if (!pausedRef.current && queueRef.current.length > 0 && !animRef.current) {
        playNextBatch();
      }
      requestAnimationFrame(loop);
    };
    const id = requestAnimationFrame(loop);
    return () => { cancelAnimationFrame(id); if (animRef.current) cancelAnimationFrame(animRef.current); };
  }, []);

  return (
    <div className="hScroller" onMouseEnter={() => (pausedRef.current = true)} onMouseLeave={() => (pausedRef.current = false)}>
      <div ref={rowRef} className="hScrollerRow">
        {items.map((b) => (
          <a key={b.height} className="hBlockCard" href={`/blocks/${b.height}`}>
            <h4>Block {b.height}</h4>
            <div className="hBlockMeta">
              <span>{b.txs ?? 0} tx</span>
              <span>{b.timestamp ? new Date(b.timestamp).toLocaleTimeString() : '—'}</span>
            </div>
          </a>
        ))}
      </div>
    </div>
  );
}
