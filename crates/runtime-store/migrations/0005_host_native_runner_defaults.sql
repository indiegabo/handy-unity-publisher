UPDATE build_targets
SET runner_type = 'host-native'
WHERE LOWER(TRIM(COALESCE(runner_type, ''))) NOT IN (
   'host-native',
   'host-windows-unity',
   'host-macos-unity',
   'host-linux-unity'
);

UPDATE build_runs
SET image_ref = 'host-native'
WHERE LOWER(TRIM(COALESCE(image_ref, ''))) NOT IN (
   'host-native',
   'host-windows-unity',
   'host-macos-unity',
   'host-linux-unity'
);