import LogTable from "../components/LogTable";

export default function LoggerPanel() {
  return (
    <div className="panel" role="tabpanel" aria-label="Logger" id="panel-logger">
      <LogTable limit={500} />
    </div>
  );
}
