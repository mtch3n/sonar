import {
  AppWindow,
  Archive,
  Calculator,
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
  PackagePlus,
  Presentation,
  Puzzle,
  Sheet,
  SquareTerminal,
} from "lucide-react";

/** File kinds from sonar-core's `Kind`, plus the glyphs plugins and the calculator use. */
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
