-- Upgrade only the previous default; leave explicitly customized keys intact.
UPDATE settings SET value_json=json_set(value_json,'$.shortcutFloating','Alt+Q'),
    updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
WHERE key='window_config' AND json_valid(value_json)
  AND json_extract(value_json,'$.shortcutFloating') IN ('CmdOrCtrl+Alt+Q','Ctrl+Alt+Q');
