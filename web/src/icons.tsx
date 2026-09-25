const PATHS: Record<string, JSX.Element> = {
  logo: <path d="M2 14c5-6 13-7 20-3-7-1-13 1-20 3zM3 18c6-2 11-2 17 0M6 7c4-1 8-1 12 1" />,
  airfoil: <path d="M3 13c3-5 11-6 18-2-6 1-12 2-18 2z" />,
  polygon: <path d="M5 18c-2-5 1-11 6-12s9 3 8 8-4 5-7 5-5 1-7-1z" />,
  brush: <><path d="M14 4l6 6-8 8H6v-6z" /><path d="M11 7l6 6" /></>,
  circle: <circle cx="12" cy="12" r="7" />,
  rect: <rect x="5" y="6" width="14" height="12" rx="1.5" />,
  erase: <><path d="M16 4l5 5-10 10H6l-3-3z" /><path d="M9 11l5 5M13 20h8" /></>,
  trash: <path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" />,
  play: <path d="M7 5l12 7-12 7z" />,
  pause: <path d="M8 5v14M16 5v14" />,
  step: <path d="M6 5l9 7-9 7zM18 5v14" />,
  reset: <><path d="M4 12a8 8 0 1 0 2.5-5.8" /><path d="M4 4v4h4" /></>,
  info: <><circle cx="12" cy="12" r="9" /><path d="M12 11v6M12 7.5v.5" /></>,
  gear: <><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" /></>,
  chart: <path d="M3 3v18h18M7 15l4-5 3 3 5-7" />,
  layers: <><path d="M12 3l9 5-9 5-9-5z" /><path d="M3 13l9 5 9-5" /></>,
  grid: <><rect x="3" y="3" width="7" height="7" rx="1" /><rect x="14" y="3" width="7" height="7" rx="1" /><rect x="3" y="14" width="7" height="7" rx="1" /><rect x="14" y="14" width="7" height="7" rx="1" /></>,
  help: <><circle cx="12" cy="12" r="9" /><path d="M9.5 9a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6v.6M12 17v.3" /></>,
  eye: <><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z" /><circle cx="12" cy="12" r="3" /></>,
  eyeoff: <><path d="M3 3l18 18M10.6 5.1A10 10 0 0 1 12 5c6.5 0 10 7 10 7a17 17 0 0 1-3.2 4M6.6 6.6C3.8 8.4 2 12 2 12s3.5 7 10 7c1.8 0 3.4-.5 4.8-1.3" /></>,
  fullscreen: <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />,
  close: <path d="M6 6l12 12M18 6L6 18" />,
  minus: <path d="M6 12h12" />,
  expand: <path d="M6 9l6 6 6-6" />,
  speed: <path d="M3 12h10M3 7h14M3 17h7M17 13l4-1-4-1" />,
  vorticity: <path d="M12 12a2 2 0 1 1 2-2 4 4 0 1 1-4-4 6 6 0 1 1-6 6" />,
  pressure: <><circle cx="12" cy="12" r="8" /><path d="M12 12l4-4M12 7v1M7 12h1M16 12h1" /></>,
  none: <><rect x="3" y="5" width="18" height="14" rx="2" /><path d="M3 19L21 5" /></>,
  particles: <><circle cx="5" cy="8" r="1" /><circle cx="11" cy="6" r="1" /><circle cx="17" cy="9" r="1" /><circle cx="7" cy="15" r="1" /><circle cx="14" cy="14" r="1" /><circle cx="19" cy="17" r="1" /></>,
  smoke: <path d="M3 8c3 0 3 2 6 2s3-2 6-2 3 2 6 2M3 14c3 0 3 2 6 2s3-2 6-2 3 2 6 2" />,
  wind: <path d="M3 8h11a3 3 0 1 0-3-3M3 12h15a3 3 0 1 1-3 3M3 16h7" />,
  cpu: <><rect x="6" y="6" width="12" height="12" rx="2" /><path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4" /></>,
};

export function Icon({ name }: { name: string }) {
  return (
    <svg className="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.7} strokeLinecap="round" strokeLinejoin="round">
      {PATHS[name]}
    </svg>
  );
}
