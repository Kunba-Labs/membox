/* Mock library. Stands in for the Rust core until it exists — the shapes here
   are exactly docs/Feature-Spec.md §2.1/§2.2, so swapping in real rows is a
   data-source change, not a UI change. */

export const collections = [
  { id: "all", name: "All", icon: "Inbox", count: 1284 },
  { id: "uncategorized", name: "Uncategorized", icon: "FolderQ", count: 37 },
  { id: "untagged", name: "Untagged", icon: "TagQ", count: 216 },
  { id: "tags", name: "All Tags", icon: "Bookmark", count: 94 },
  { id: "trash", name: "Trash", icon: "Trash", count: 12 },
];

export const smartFolders = [
  { id: "sf-week", name: "Added this week", count: 41 },
  { id: "sf-fav", name: "Five stars", count: 28 },
  { id: "sf-tx", name: "Has transcript", count: 163 },
  { id: "sf-pending", name: "Needs review", count: 6 },
];

export const folders = [
  {
    id: "f-watch",
    name: "Watching",
    emoji: "🎬",
    count: 412,
    children: [
      { id: "f-docs", name: "Documentaries", count: 118 },
      { id: "f-talks", name: "Talks", count: 96 },
      { id: "f-tut", name: "Tutorials", count: 143 },
      { id: "f-film", name: "Film", count: 55 },
    ],
  },
  {
    id: "f-read",
    name: "Reading",
    emoji: "📚",
    count: 356,
    children: [
      { id: "f-longform", name: "Longform", count: 141 },
      { id: "f-docs2", name: "Docs & Refs", count: 132 },
      { id: "f-news", name: "Newsletters", count: 83 },
    ],
  },
  {
    id: "f-listen",
    name: "Listening",
    emoji: "🎧",
    count: 187,
    children: [
      { id: "f-sets", name: "DJ Sets", count: 74 },
      { id: "f-albums", name: "Albums", count: 113 },
    ],
  },
  {
    id: "f-travel",
    name: "Travel",
    emoji: "🌍",
    count: 164,
    children: [
      { id: "f-za", name: "South Africa", count: 61 },
      { id: "f-jp", name: "Japan", count: 58 },
      { id: "f-pt", name: "Portugal", count: 45 },
    ],
  },
  {
    id: "f-build",
    name: "Build",
    emoji: "🛠",
    count: 165,
    children: [
      { id: "f-rust", name: "Rust", count: 88 },
      { id: "f-ds", name: "Design Systems", count: 77 },
    ],
  },
];

const raw = [
  ["Cape Town from Lion's Head", "youtube_video", "youtube.com", 3, 4, ["travel", "south africa"], "12:04"],
  ["The Garden Route, end to end", "youtube_video", "youtube.com", 16, 9, ["travel", "documentary"], "48:31"],
  ["Why Karoo light looks like that", "webpage", "nautil.us", 3, 4, ["photography"], null],
  ["Ndlovu Youth Choir — Live", "youtube_music", "music.youtube.com", 1, 1, ["music", "live"], "5:42"],
  ["Table Mountain trail notes", "webpage", "hikingsouthafrica.co.za", 4, 5, ["travel", "hiking"], null],
  ["Braai basics, properly", "instagram", "instagram.com", 4, 5, ["food"], null],
  ["Rust ownership, finally", "youtube_video", "youtube.com", 16, 9, ["rust", "tutorial"], "22:15"],
  ["Sea Point promenade, golden hour", "image", "unsplash.com", 2, 3, ["photography"], null],
  ["Kruger at 5am", "youtube_video", "youtube.com", 16, 9, ["travel", "wildlife"], "31:08"],
  ["A note on error types", "snippet", "Notes.app", 4, 3, ["rust"], null],
  ["Amapiano, an origin story", "youtube_music", "music.youtube.com", 1, 1, ["music"], "9:20"],
  ["Design tokens that survive", "webpage", "figma.com", 3, 4, ["design systems"], null],
  ["Wine estates, Stellenbosch", "instagram", "instagram.com", 4, 5, ["travel", "food"], null],
  ["sqlite-vec in anger", "webpage", "github.com", 16, 10, ["rust", "search"], null],
  ["Drakensberg, three days", "youtube_video", "youtube.com", 3, 4, ["travel", "hiking"], "18:47"],
  ["The colour of Johannesburg", "webpage", "longreads.com", 2, 3, ["longform"], null],
  ["Cape Point, no filter", "image", "unsplash.com", 3, 2, ["photography"], null],
  ["Kaapse Klopse", "youtube_video", "youtube.com", 16, 9, ["culture"], "7:55"],
  ["Deep house, Sunday set", "youtube_music", "music.youtube.com", 1, 1, ["music", "sets"], "1:02:11"],
  ["WKWebView snapshot gotchas", "webpage", "developer.apple.com", 4, 5, ["swift"], null],
  ["Robben Island, walkthrough", "youtube_video", "youtube.com", 16, 9, ["history"], "25:30"],
  ["Bo-Kaap, every door", "instagram", "instagram.com", 1, 1, ["photography"], null],
  ["Reading list: Southern Africa", "snippet", "Safari", 4, 3, ["longform"], null],
  ["Whale watching, Hermanus", "youtube_video", "youtube.com", 3, 4, ["travel", "wildlife"], "14:22"],
];

export const items = raw.map(([title, kind, domain, aw, ah, tags, duration], i) => ({
  id: `i-${i}`,
  title,
  kind,
  domain,
  url: `https://${domain}/${title.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`,
  thumb: `/mock/${i}.jpg`,
  pageShot: `/mock/${i}-page.jpg`,
  aspect: aw / ah,
  tags,
  // The first tag of every demo item is a machine tag, so the fallback shows the style.
  autoTags: tags.slice(0, 1),
  folders: [folders[i % folders.length].children[0].name],
  rating: [5, 4, 0, 3, 5, 0, 4][i % 7],
  duration,
  status: i === 3 ? "enriching" : i === 9 ? "pending" : "ready",
  summary:
    "Filed automatically by the enrichment agent from the page screenshot, the readable body and — for video — the transcript. Edit anything here and it is never overwritten by a later run.",
  addedAt: new Date(Date.UTC(2026, 7, 20 + (i % 5), 10 + (i % 9), 40 + (i % 6))).toISOString(),
  size: (0.4 + (i % 7) * 0.9).toFixed(2) + " MB",
  dimensions: `${aw * 320}×${ah * 320}`,
  palette: [
    ["#2c4a6e", "#3f6d99", "#7fa8cc", "#c8dcea", "#e8b98a", "#a8553a"],
    ["#1e3d34", "#2f6b57", "#67a68a", "#b8d9c6", "#e0c98f", "#8a6b3f"],
    ["#3a2a4a", "#5c4278", "#8c6fb0", "#c4aede", "#e8a0c8", "#7a3f5c"],
  ][i % 3],
}));

export const kindLabel = {
  webpage: "WEB",
  youtube_video: "VIDEO",
  youtube_music: "MUSIC",
  youtube_playlist: "PLAYLIST",
  instagram: "REEL",
  tiktok: "TIKTOK",
  x_post: "POST",
  reddit: "REDDIT",
  vimeo: "VIDEO",
  music: "MUSIC",
  github_repo: "REPO",
  pdf: "PDF",
  file: "FILE",
  image: "IMG",
  hn: "HN",
  book: "BOOK",
  game: "GAME",
  movie: "FILM",
  tv: "SERIES",
  product: "PRODUCT",
  note: "NOTE",
  snippet: "TEXT",
};
