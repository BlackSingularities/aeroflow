const PATHS: Record<string, JSX.Element> = {
  logo: <path d="M2 14c5-6 13-7 20-3-7-1-13 1-20 3zM3 18c6-2 11-2 17 0M6 7c4-1 8-1 12 1" />,
  airfoil: <path d="M3 13c3-5 11-6 18-2-6 1-12 2-18 2z" />,
  polygon: <path d="M5 18c-2-5 1-11 6-12s9 3 8 8-4 5-7 5-5 1-7-1z" />,
  brush: <><path d="M14 4l6 6-8 8H6v-6z" /><path d="M11 7l6 6" /></>,
  circle: <circle cx="12" cy="12" r="7" />,
  rect: <rect x="5" y="6" width="14" height="12" rx="1.5" />,
  erase: <><path d="M16 4l5 5-10 10H6l-3-3z" /><path d="M9 11l5 5M13 20h8" /></>,
  trash: <><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" /></>,
  play: <path d="M7 5l12 7-12 7z" />,
  pause: <path d="M8 5v14M16 5v14" />,
  step: <path d="M6 5l9 7-9 7zM18 5v14" />,
  reset: <><path d="M4 12a8 8 0 1 0 2.5-5.8" /><path d="M4 4v4h4" /></>,
  info: <><circle cx="12" cy="12" r="9" /><path d="M12 11v6M12 7.5v.5" /></>,
};

export function Icon({ name }: { name: string }) {
  return (
    <svg className="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round">
      {PATHS[name]}
    </svg>
  );
}
