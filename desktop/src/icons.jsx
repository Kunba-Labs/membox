/* Inline SVG icons. 16px grid, 1.5 stroke, currentColor — matches Eagle's weight.
   One file rather than a dependency; there are only a dozen of them. */

const s = {
  width: 16,
  height: 16,
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round",
  strokeLinejoin: "round",
};

export const Plus = (p) => (
  <svg {...s} {...p}>
    <path d="M8 3.5v9M3.5 8h9" />
  </svg>
);

export const Swap = (p) => (
  <svg {...s} {...p}>
    <path d="M2.5 5.5h11l-2.5-2.5M13.5 10.5h-11l2.5 2.5" />
  </svg>
);

export const Panel = (p) => (
  <svg {...s} {...p}>
    <rect x="2" y="3" width="12" height="10" rx="2" />
    <path d="M6.5 3v10" />
  </svg>
);

export const ChevronLeft = (p) => (
  <svg {...s} {...p}>
    <path d="M10 3.5 5.5 8l4.5 4.5" />
  </svg>
);

export const ChevronRight = (p) => (
  <svg {...s} {...p}>
    <path d="M6 3.5 10.5 8 6 12.5" />
  </svg>
);

export const ChevronDown = (p) => (
  <svg {...s} {...p}>
    <path d="M3.5 6 8 10.5 12.5 6" />
  </svg>
);

export const Caret = ({ open, ...p }) => (
  <svg {...s} {...p} strokeWidth={2}>
    <path d={open ? "M4 6.5 8 10.5 12 6.5" : "M6.5 4 10.5 8 6.5 12"} />
  </svg>
);

export const Sort = (p) => (
  <svg {...s} {...p}>
    <path d="M4.5 2.5v11M2 11l2.5 2.5L7 11M11.5 13.5v-11M9 5l2.5-2.5L14 5" />
  </svg>
);

export const Search = (p) => (
  <svg {...s} {...p}>
    <circle cx="7" cy="7" r="4" />
    <path d="m10.5 10.5 3 3" />
  </svg>
);

export const Filter = (p) => (
  <svg {...s} {...p}>
    <path d="M2.5 3.5h11l-4.25 5v4.5l-2.5-1.5V8.5z" />
  </svg>
);

export const Layout = (p) => (
  <svg {...s} {...p}>
    <rect x="2" y="3" width="12" height="10" rx="2" />
    <path d="M2 6.5h12M6.5 6.5V13" />
  </svg>
);

export const Bolt = (p) => (
  <svg {...s} {...p}>
    <path d="M9 2 4 9h3.5L7 14l5-7H8.5z" />
  </svg>
);

export const Puzzle = (p) => (
  <svg {...s} {...p}>
    <path d="M6 2.5a1.5 1.5 0 0 1 3 0V4h2.5a1 1 0 0 1 1 1v2.5H14a1.5 1.5 0 0 1 0 3h-1.5V13a1 1 0 0 1-1 1H9v-1.5a1.5 1.5 0 0 0-3 0V14H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1H6z" />
  </svg>
);

export const Pin = (p) => (
  <svg {...s} {...p}>
    <path d="M6 2h4l-.5 4 2 2.5H4.5L6.5 6z" />
    <path d="M8 8.5V14" />
  </svg>
);

export const Link = (p) => (
  <svg {...s} {...p}>
    <path d="M6.5 9.5a2.5 2.5 0 0 0 3.5 0l2-2a2.475 2.475 0 0 0-3.5-3.5l-1 1" />
    <path d="M9.5 6.5a2.5 2.5 0 0 0-3.5 0l-2 2a2.475 2.475 0 0 0 3.5 3.5l1-1" />
  </svg>
);

export const Close = (p) => (
  <svg {...s} {...p} strokeWidth={1.3}>
    <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" />
  </svg>
);

export const Inbox = (p) => (
  <svg {...s} {...p}>
    <path d="M2 9.5 3.5 3h9L14 9.5V13H2z" />
    <path d="M2 9.5h3.5l1 1.5h3l1-1.5H14" />
  </svg>
);

export const FolderQ = (p) => (
  <svg {...s} {...p}>
    <path d="M2 12.5V4a1 1 0 0 1 1-1h3l1.5 1.5H13a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1Z" />
  </svg>
);

export const TagQ = (p) => (
  <svg {...s} {...p}>
    <path d="M8 2.5 13.5 8 8 13.5 2.5 8z" />
  </svg>
);

export const Bookmark = (p) => (
  <svg {...s} {...p}>
    <path d="M4 2.5h8v11l-4-3-4 3z" />
  </svg>
);

export const Trash = (p) => (
  <svg {...s} {...p}>
    <path d="M2.5 4h11M6 4V2.5h4V4M4 4l.75 9.5h6.5L12 4" />
  </svg>
);

export const Folder = (p) => (
  <svg {...s} {...p}>
    <path d="M2 12.5V4a1 1 0 0 1 1-1h3l1.5 1.5H13a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1Z" />
  </svg>
);

export const Play = (p) => (
  <svg {...s} {...p} fill="currentColor" stroke="none">
    <path d="M5 3.5v9l7.5-4.5z" />
  </svg>
);

export const Star = ({ filled, ...p }) => (
  <svg {...s} {...p} fill={filled ? "currentColor" : "none"}>
    <path d="m8 2.5 1.7 3.5 3.8.5-2.75 2.65.65 3.85L8 11.2 4.6 13l.65-3.85L2.5 6.5l3.8-.5z" />
  </svg>
);

export const Spinner = (p) => (
  <svg {...s} {...p}>
    <circle cx="8" cy="8" r="5.5" opacity=".2" />
    <path d="M8 2.5a5.5 5.5 0 0 1 5.5 5.5" />
  </svg>
);

// A cog, not a sun. The body ring stops the teeth reading as rays, and the
// teeth are stubby and heavier than the house stroke so they read as teeth.
export const Gear = (p) => (
  <svg {...s} {...p}>
    <circle cx="8" cy="8" r="4.6" />
    <circle cx="8" cy="8" r="1.5" />
    <path strokeWidth="2.1" d="M8 2.1v1.1M8 12.8v1.1M2.1 8h1.1M12.8 8h1.1M3.9 3.9l.8.8M11.3 11.3l.8.8M3.9 12.1l.8-.8M11.3 4.7l.8-.8" />
  </svg>
);

export const PanelRight = (p) => (
  <svg {...s} {...p}>
    <rect x="2" y="3" width="12" height="10" rx="2" />
    <path d="M9.5 3v10" />
  </svg>
);

// Brand marks keep their colour — the point of them is recognition.
export const HN = (p) => (
  <svg {...s} fill="none" {...p}>
    <rect x="1.5" y="1.5" width="13" height="13" rx="3" fill="#ff6600" stroke="none" />
    <path d="M8 11.4V8.5L5.5 4.4h1.7L8 6.9l1.3-2.5H11L8.5 8.5v2.9z" fill="#fff" stroke="none" />
  </svg>
);

export const Reddit = (p) => (
  <svg {...s} fill="none" {...p}>
    <circle cx="8" cy="8" r="6.4" fill="#ff4500" stroke="none" />
    <circle cx="5.9" cy="7.7" r="0.95" fill="#fff" stroke="none" />
    <circle cx="10.1" cy="7.7" r="0.95" fill="#fff" stroke="none" />
    <path d="M5.7 10.2c1.3.9 3.3.9 4.6 0" stroke="#fff" strokeWidth="1.1" />
  </svg>
);

export const Headphones = (p) => (
  <svg {...s} {...p}>
    <path d="M2.8 10.2V8.4a5.2 5.2 0 0 1 10.4 0v1.8" />
    <path d="M2.8 9.6h1.4a.8.8 0 0 1 .8.8v2.2a.8.8 0 0 1-.8.8H3.6a.8.8 0 0 1-.8-.8zM13.2 9.6h-1.4a.8.8 0 0 0-.8.8v2.2a.8.8 0 0 0 .8.8h.6a.8.8 0 0 0 .8-.8z" />
  </svg>
);

export const Globe = (p) => (
  <svg {...s} {...p}>
    <circle cx="8" cy="8" r="5.8" />
    <path d="M2.3 8h11.4M8 2.2c1.6 1.7 2.4 3.7 2.4 5.8S9.6 12.1 8 13.8C6.4 12.1 5.6 10.1 5.6 8s.8-4.1 2.4-5.8Z" />
  </svg>
);

export const Tools = (p) => (
  <svg {...s} {...p}>
    <path d="M13 3.1a3 3 0 0 1-3.9 3.9l-4.6 4.6a1.6 1.6 0 1 1-2.2-2.2L6.9 4.8A3 3 0 0 1 10.8 1L8.9 2.9l.6 2.1 2.1.6z" />
  </svg>
);

export const Sticky = (p) => (
  <svg {...s} {...p}>
    <path d="M2.6 3.4a.8.8 0 0 1 .8-.8h9.2a.8.8 0 0 1 .8.8v5.9L9.3 13.4H3.4a.8.8 0 0 1-.8-.8z" />
    <path d="M13.4 9.2H10a.7.7 0 0 0-.7.7v3.5" />
  </svg>
);

export const Cart = (p) => (
  <svg {...s} {...p}>
    <path d="M1.8 2.6h1.9l1.7 7.1h6.4l1.4-5H4.3" />
    <circle cx="6" cy="12.6" r="1.1" />
    <circle cx="11.4" cy="12.6" r="1.1" />
  </svg>
);

export const Game = (p) => (
  <svg {...s} {...p}>
    <path d="M5.4 4.5h5.2a3.4 3.4 0 0 1 3.3 2.6l.7 3.2a1.8 1.8 0 0 1-3.2 1.5l-1-1.3H5.6l-1 1.3a1.8 1.8 0 0 1-3.2-1.5l.7-3.2a3.4 3.4 0 0 1 3.3-2.6Z" />
    <path d="M4.6 7.4v1.8M3.7 8.3h1.8M10.6 7.9h.01M12 9.2h.01" />
  </svg>
);

export const Book = (p) => (
  <svg {...s} {...p}>
    <path d="M3 3.2h4.2a1.6 1.6 0 0 1 1.6 1.6v8.2a1.2 1.2 0 0 0-1.2-1.2H3z" />
    <path d="M13.4 3.2H9.2a1.6 1.6 0 0 0-1.6 1.6v8.2a1.2 1.2 0 0 1 1.2-1.2h4.6z" />
  </svg>
);

export const Film = (p) => (
  <svg {...s} {...p}>
    <rect x="1.8" y="3.4" width="12.4" height="9.2" rx="1.6" />
    <path d="M1.8 6.4h12.4M4.6 3.4v3M9.4 3.4v3M4.6 9.6h6.8" />
  </svg>
);

export const Tag = (p) => (
  <svg {...s} {...p}>
    <path d="M8.2 1.9H14v5.8l-6.3 6.3a1.2 1.2 0 0 1-1.7 0L1.9 9.9a1.2 1.2 0 0 1 0-1.7z" />
    <circle cx="11.2" cy="4.8" r="1.05" />
  </svg>
);

export const Warn = (p) => (
  <svg {...s} {...p}>
    <path d="M8 2.6 14.3 13H1.7z" />
    <path d="M8 6.6v3.1M8 11.4v.1" />
  </svg>
);

export const Check = (p) => (
  <svg {...s} {...p} strokeWidth={2}>
    <path d="M3.5 8.5 6.5 11.5 12.5 4.5" />
  </svg>
);
