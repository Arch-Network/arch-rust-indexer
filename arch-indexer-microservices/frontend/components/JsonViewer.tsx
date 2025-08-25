import React, { useMemo, useState } from 'react';
import ReactJson from '@microlink/react-json-view';

type JsonValue = string | number | boolean | null | JsonObject | JsonArray;
type JsonObject = { [key: string]: JsonValue };
type JsonArray = JsonValue[];

type JsonViewerProps = {
  data: any;
  initiallyExpanded?: boolean;
  collapseAfter?: number;
};

function isPrimitive(value: JsonValue): value is string | number | boolean | null {
  return (
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'boolean' ||
    value === null
  );
}

function summarize(value: JsonValue): string {
  if (isPrimitive(value)) {
    if (typeof value === 'string') return `"${value}"`;
    return String(value);
  }
  if (Array.isArray(value)) return `Array(${value.length})`;
  return `Object(${Object.keys(value).length})`;
}

export default function JsonViewer({ data, initiallyExpanded = true, collapseAfter = 200 }: JsonViewerProps) {
  return (
    <div className="jsonViewerRoot">
      <ReactJson
        src={data}
        name={false}
        theme={{
          base00: '#0a0c10',
          base01: '#0f1116',
          base02: '#151922',
          base03: '#1b2230',
          base04: '#a1aab8',
          base05: '#e6edf3',
          base06: '#e6edf3',
          base07: '#ffffff',
          base08: '#9cdcfe',
          base09: '#b5cea8',
          base0A: '#dcdcaa',
          base0B: '#c586c0',
          base0C: '#19e3ff',
          base0D: '#7aa2f7',
          base0E: '#19e3ff',
          base0F: '#f97583'
        } as any}
        collapsed={false}
        collapseStringsAfterLength={120}
        displayDataTypes={false}
        displayObjectSize={false}
        enableClipboard={true}
        indentWidth={2}
        iconStyle="triangle"
        style={{ background: 'transparent' }}
        groupArraysAfterLength={collapseAfter}
      />
    </div>
  );
}

type JsonNodeProps = {
  value: JsonValue;
  path: string;
  level: number;
  initiallyExpanded: boolean;
  collapseAfter: number;
  name?: string;
};

function JsonNode({ value, path, level, initiallyExpanded, collapseAfter, name }: JsonNodeProps) {
  const [expanded, setExpanded] = useState(level === 0 ? true : initiallyExpanded);

  if (isPrimitive(value)) {
    const typeName = value === null ? 'null' : typeof value;
    const rendered = typeof value === 'string' ? `"${value}"` : String(value);
    return (
      <div className="jsonLine">
        {name !== undefined && <span className="jsonKey">{name}: </span>}
        <span className="jsonTypeLabel">{typeName}</span>{' '}
        <span className={`jsonPrimitive json-${typeof value}`}>{rendered}</span>
      </div>
    );
  }

  if (Array.isArray(value)) {
    const shouldCollapse = value.length > collapseAfter;
    const items = value;
    return (
      <div className="jsonBlock">
        <div className="jsonHeader" onClick={() => setExpanded((v) => !v)}>
          <span className="jsonChevron">{expanded ? '▾' : '▸'}</span>
          {name !== undefined && <span className="jsonKey">{name}: </span>}
          <span className="jsonType">[{items.length}]</span>
          <span className="jsonArrayBadge"> Array({items.length})</span>
          {!expanded && <span className="jsonSummary"> {summarize(value)}</span>}
        </div>
        {expanded && (
          <div className="jsonChildren">
            {shouldCollapse ? (
              <div className="jsonLine jsonMuted">Large array collapsed ({items.length} items)</div>
            ) : (
              items.map((item, idx) => (
                <JsonNode
                  key={`${path}.${idx}`}
                  value={item}
                  path={`${path}.${idx}`}
                  level={level + 1}
                  initiallyExpanded={initiallyExpanded}
                  collapseAfter={collapseAfter}
                  name={`${idx}`}
                />
              ))
            )}
          </div>
        )}
      </div>
    );
  }

  const entries = Object.entries(value);
  return (
    <div className="jsonBlock">
      <div className="jsonHeader" onClick={() => setExpanded((v) => !v)}>
        <span className="jsonChevron">{expanded ? '▾' : '▸'}</span>
        {name !== undefined && <span className="jsonKey">{name}: </span>}
        <span className="jsonType">{'{'}{entries.length}{'}'}</span>
        {!expanded && <span className="jsonSummary"> {summarize(value)}</span>}
      </div>
      {expanded && (
        <div className="jsonChildren">
          {entries.map(([k, v]) => (
            <JsonNode
              key={`${path}.${k}`}
              value={v}
              path={`${path}.${k}`}
              level={level + 1}
              initiallyExpanded={initiallyExpanded}
              collapseAfter={collapseAfter}
              name={k}
            />
          ))}
        </div>
      )}
    </div>
  );
}
