export const middleEllipsis = (value: string, keep: number = 8): string => {
  if (!value || typeof value !== 'string') return '';
  if (value.length <= keep * 2 + 1) return value;
  return `${value.slice(0, keep)}…${value.slice(-keep)}`;
};

// Parse timestamps that may be naive (no timezone) as UTC
export const parseTimestampUtc = (value: string | null | undefined): Date | null => {
  if (!value || typeof value !== 'string') return null;
  const hasZone = /Z|[+-]\d{2}:?\d{2}$/.test(value);
  const iso = hasZone ? value : `${value}Z`;
  const d = new Date(iso);
  return isNaN(d.getTime()) ? null : d;
};

export const formatDateTime = (
  value: string | Date | null | undefined,
  options?: { timeZone?: string | 'local'; includeZone?: boolean }
): string => {
  try {
    const includeZone = options?.includeZone !== false;
    const tz = options?.timeZone && options.timeZone !== 'local' ? options.timeZone : undefined;
    const d = typeof value === 'string' ? parseTimestampUtc(value) : (value instanceof Date ? (isNaN(value.getTime()) ? null : value) : null);
    if (!d) return 'INVALID DATE';
    return d.toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      hour12: true,
      timeZone: tz,
      ...(includeZone ? { timeZoneName: 'short' as const } : {}),
    }).toUpperCase();
  } catch {
    return 'FORMAT ERROR';
  }
};
