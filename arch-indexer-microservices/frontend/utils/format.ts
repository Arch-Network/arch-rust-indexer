export const middleEllipsis = (value: string, keep: number = 8): string => {
  if (!value || typeof value !== 'string') return '';
  if (value.length <= keep * 2 + 1) return value;
  return `${value.slice(0, keep)}…${value.slice(-keep)}`;
};
