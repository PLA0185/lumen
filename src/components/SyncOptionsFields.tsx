import { SYNC_SCOPES, SYNC_DIRECTIONS, type SyncOptions } from '../lib/cloud-sync-ipc'
export function SyncOptionsFields({ value, onChange, prefix }: { value: SyncOptions; onChange: (value: SyncOptions) => void; prefix: '本次' | '默认' }) {
  return <div className="formgrid">
    <label className="formrow"><span className="formlabel">{prefix}同步内容</span><select className="input" aria-label={`${prefix}同步内容`} value={value.scope} onChange={e => onChange({ ...value, scope: e.target.value as SyncOptions['scope'] })}>
      {Object.entries(SYNC_SCOPES).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
    </select></label>
    <label className="formrow"><span className="formlabel">{prefix}同步方向</span><select className="input" aria-label={`${prefix}同步方向`} value={value.direction} onChange={e => onChange({ ...value, direction: e.target.value as SyncOptions['direction'] })}>
      {Object.entries(SYNC_DIRECTIONS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
    </select></label>
  </div>
}
