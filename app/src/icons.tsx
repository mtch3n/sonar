import {
  AppWindow,
  Archive,
  Bookmark,
  Calculator,
  Clipboard,
  Clock,
  Cpu,
  EthernetPort,
  File,
  FileCode,
  FileCog,
  FileText,
  Film,
  Folder,
  FolderGit2,
  Globe,
  History,
  Image,
  KeyRound,
  Lock,
  LogOut,
  type LucideIcon,
  Moon,
  Music,
  PackagePlus,
  Power,
  Presentation,
  Puzzle,
  RotateCcw,
  Server,
  Sheet,
  Smile,
  SquareTerminal,
  Trash2,
} from "lucide-react";

/** File kinds from sonar-core's `Kind`, plus the glyphs plugins, the calculator and browser results use. */
const GLYPHS: Record<string, LucideIcon> = {
  project: FolderGit2,
  folder: Folder,
  app: AppWindow,
  code: FileCode,
  script: SquareTerminal,
  key: KeyRound,
  pdf: FileText,
  doc: FileText,
  sheet: Sheet,
  slides: Presentation,
  image: Image,
  video: Film,
  audio: Music,
  archive: Archive,
  config: FileCog,
  other: File,
  calculator: Calculator,
  bookmark: Bookmark,
  history: History,
  window: AppWindow,
  process: Cpu,
  port: EthernetPort,
  service: Server,
  power: Power,
  lock: Lock,
  sleep: Moon,
  restart: RotateCcw,
  logout: LogOut,
  trash: Trash2,
  terminal: SquareTerminal,
  clock: Clock,
  globe: Globe,
  clipboard: Clipboard,
  emoji: Smile,
  plugin: Puzzle,
  github: PackagePlus,
};

export function Glyph({ icon, image }: { icon: string; image: string | null }) {
  if (image) {
    return (
      <span className="glyph">
        <img src={image} alt="" />
      </span>
    );
  }
  const Icon = GLYPHS[icon] ?? File;
  return (
    <span className="glyph">
      <Icon size={20} strokeWidth={1.75} aria-hidden />
    </span>
  );
}
