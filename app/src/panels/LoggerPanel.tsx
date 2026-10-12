import LogTable from "../components/LogTable";

export default function LoggerPanel({ focus }: { focus?: { id: number; seq: number } | null }) {
  return (
    <div className="panel" role="tabpanel" aria-label="Logger" id="panel-logger">
      <LogTable limit={500} focus={focus} />
    </div>
  );
}
