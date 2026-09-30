import type { ReactNode } from "react";
import "./PageHeader.css";

/** `FLPageHeader`: eyebrow + 32pt bold title, optional trailing accessory. */
export function PageHeader({ eyebrow, title, accessory }: { eyebrow: string; title: string; accessory?: ReactNode }) {
  return (
    <div className="page-header">
      <div className="eyebrow">{eyebrow}</div>
      <div className="page-header__row">
        <h1 className="page-header__title">{title}</h1>
        {accessory && <div className="page-header__accessory">{accessory}</div>}
      </div>
    </div>
  );
}
