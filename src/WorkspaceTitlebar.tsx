import { useRef, useState, type ReactNode } from "react";
import { AppBrand } from "./AppBrand";
import { WindowTitlebar } from "./WindowTitlebar";
import { WorkspaceTabs, type WorkspaceTabsProps } from "./WorkspaceTabs";

export function WorkspaceTitlebar({
  actions,
  ...tabs
}: WorkspaceTabsProps & { actions?: ReactNode }) {
  const appendDropRef = useRef<HTMLDivElement>(null);
  const [appendDropActive, setAppendDropActive] = useState(false);
  return (
    <WindowTitlebar
      leading={<AppBrand iconOnly />}
      tabs={
        <WorkspaceTabs
          {...tabs}
          appendDropRef={appendDropRef}
          onAppendDropChange={setAppendDropActive}
        />
      }
      dragRegionRef={appendDropRef}
      appendDropActive={appendDropActive}
      actions={actions}
      onError={tabs.onError}
    />
  );
}
