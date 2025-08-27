import React from 'react';

type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'icon';
type ButtonSize = 'sm' | 'md';

type Props = React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
};

const baseStyle: React.CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 8,
  border: '1px solid var(--accent)',
  background: 'var(--panel)',
  color: 'var(--text)',
  cursor: 'pointer',
  fontWeight: 600,
  letterSpacing: 1,
  textTransform: 'uppercase',
  boxShadow: '0 4px 16px rgba(0,0,0,0.35)',
};

const sizes: Record<ButtonSize, React.CSSProperties> = {
  sm: { padding: '8px 12px', fontSize: 12 },
  md: { padding: '12px 20px', fontSize: 13 },
};

const variants: Record<ButtonVariant, React.CSSProperties> = {
  primary: {
    background: 'var(--panel)',
    borderColor: 'var(--accent)',
  },
  secondary: {
    background: '#0a0c10',
    borderColor: 'rgba(255,255,255,0.12)',
  },
  ghost: {
    background: 'transparent',
    borderColor: 'rgba(255,255,255,0.12)',
  },
  icon: {
    background: 'var(--panel)',
    borderColor: 'rgba(255,255,255,0.12)',
    padding: 8,
  },
};

export default function Button({ variant = 'primary', size = 'md', style, disabled, ...rest }: Props) {
  const finalStyle: React.CSSProperties = {
    ...baseStyle,
    ...sizes[size],
    ...variants[variant],
    ...(disabled ? { opacity: 0.6, cursor: 'not-allowed' } : {}),
    ...style,
  };

  return (
    <button {...rest} style={finalStyle} disabled={disabled} />
  );
}

