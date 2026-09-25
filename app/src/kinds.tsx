import {
  AppWindow,
  Archive,
  File,
  FileCode,
  FileCog,
  FileText,
  Film,
  Folder,
  FolderGit2,
  Image,
  KeyRound,
  type LucideIcon,
  Music,
  Presentation,
  Sheet,
  SquareTerminal,
} from "lucide-react";

/** One icon per kind from sonar-core's `Kind`. Colors live in styles.css. */
const ICONS: Record<string, LucideIcon> = {
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
};

export function KindIcon({ kind }: { kind: string }) {
  const Icon = ICONS[kind] ?? File;
  return (
    <span className={`kind kind-${kind}`} title={kind}>
      <Icon size={18} strokeWidth={2} />
    </span>
  );
}
