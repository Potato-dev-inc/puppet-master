// Settings > Appearance: the "Animated tray icon" switch (backend pref `ambient.json`, default on).
import { useEffect, useState } from 'react';
import { cockpitApi } from '../../../lib/cockpit-api';
import { SettingToggle } from '../settings-ui';

export function AmbientSettings() {
  const [animated, setAnimated] = useState(true);

  useEffect(() => {
    let cancelled = false;
    cockpitApi
      .ambientGetPrefs()
      .then((prefs) => {
        if (!cancelled) setAnimated(prefs.animated_tray_icon);
      })
      .catch(() => {
        /* not running in the desktop shell: keep the default */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <SettingToggle
      label="Animated tray icon"
      description="The tray icon shows a moving dot pattern while agents are working. Turn this off for a still icon if motion is distracting."
      checked={animated}
      implemented
      onChange={(value) => {
        setAnimated(value);
        void cockpitApi.ambientSetAnimatedTray(value).catch(() => setAnimated(!value));
      }}
    />
  );
}
