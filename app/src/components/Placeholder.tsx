type Props = {
  id?: string;
  title: string;
  description: string;
  art?: React.ReactNode;
  actions?: React.ReactNode;
  staged?: string;
};

// Burp-style blank state: illustration, heading, description, actions.
export default function Placeholder({ id, title, description, art, actions, staged }: Props) {
  return (
    <div
      className="blank"
      role="tabpanel"
      aria-label={title}
      id={id}
      tabIndex={0}
    >
      <div className="blank-inner">
        <div className="blank-art" aria-hidden>
          {art ?? <DefaultArt />}
        </div>
        <h2>{title}</h2>
        <p>{description}</p>
        {actions && <div className="blank-actions">{actions}</div>}
        {staged && <p className="placeholder-staged">{staged}</p>}
      </div>
    </div>
  );
}

function DefaultArt() {
  return (
    <svg width="96" height="72" viewBox="0 0 96 72" fill="none" xmlns="http://www.w3.org/2000/svg">
      <rect x="8" y="14" width="34" height="24" rx="4" fill="#2a78bd" transform="rotate(-8 8 14)" />
      <rect x="54" y="34" width="34" height="24" rx="4" fill="#2a78bd" transform="rotate(8 54 34)" />
      <rect x="14" y="19" width="10" height="4" rx="1.5" fill="#0e1116" transform="rotate(-8 14 19)" />
      <rect x="60" y="39" width="10" height="4" rx="1.5" fill="#0e1116" transform="rotate(8 60 39)" />
      <path d="M40 30 L56 40" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" />
      <path d="M56 30 L40 40" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" />
    </svg>
  );
}
