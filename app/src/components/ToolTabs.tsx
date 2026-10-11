import { Search, Settings } from "lucide-react";
import { TabId } from "../types";
import { tabListKeyDown } from "./tabKeys";

const TABS: [TabId, string][] = [
  ["dashboard", "Dashboard"],
  ["target", "Target"],
  ["proxy", "Proxy"],
  ["intruder", "Intruder"],
  ["repeater", "Repeater"],
  ["comparer", "Comparer"],
  ["collaborator", "Collaborator"],
  ["sequencer", "Sequencer"],
  ["decoder", "Decoder"],
  ["logger", "Logger"],
  ["extensions", "Extensions"],
];

type Props = {
  tab: TabId;
  setTab: (t: TabId) => void;
  onSearch: () => void;
  onSettings: () => void;
};

export default function ToolTabs({ tab, setTab, onSearch, onSettings }: Props) {
  const activeIdx = TABS.findIndex(([id]) => id === tab);
  const rovingIdx = activeIdx === -1 ? 0 : activeIdx;

  return (
    <div className="tooltabs">
      <div className="tooltab-list" role="tablist" aria-label="Tools">
        {TABS.map(([id, label], i) => (
          <button
            key={id}
            role="tab"
            aria-selected={tab === id}
            aria-controls={`panel-${id}`}
            tabIndex={rovingIdx === i ? 0 : -1}
            className={`tab ${tab === id ? "active" : ""}`}
            onClick={() => setTab(id)}
            onKeyDown={(e) =>
              tabListKeyDown(e, i, TABS.map(() => false), (next) => setTab(TABS[next][0]))
            }
          >
            {label}
          </button>
        ))}
      </div>
      <div className="tools-right">
        <button className="icon-btn" title="Search (Phase 2)" disabled onClick={onSearch}>
          <Search size={16} />
        </button>
        <button
          className={`icon-btn ${tab === "settings" ? "active" : ""}`}
          aria-current={tab === "settings" ? "page" : undefined}
          title="Settings"
          onClick={onSettings}
        >
          <Settings size={16} />
        </button>
      </div>
    </div>
  );
}
