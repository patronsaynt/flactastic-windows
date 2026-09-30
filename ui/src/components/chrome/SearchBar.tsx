import { Search } from "lucide-react";
import "./SearchBar.css";

/** `SearchBarView`: `.capsule` (34pt pill) or `.rounded` (8pt radius). */
export function SearchBar({
  value,
  onChange,
  style = "capsule",
}: {
  value: string;
  onChange: (v: string) => void;
  style?: "capsule" | "rounded";
}) {
  return (
    <label className={"search search--" + style + (style === "capsule" ? " capsule" : "")}>
      <Search size={13} strokeWidth={2.2} />
      <input
        value={value}
        placeholder={style === "capsule" ? "Search…" : "Search..."}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") (e.target as HTMLInputElement).blur();
        }}
      />
    </label>
  );
}
