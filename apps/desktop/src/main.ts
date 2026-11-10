// Placeholder page: shows the app state. The real UI replaces this.
import { getSettings, getState, onCameraEvent } from './ipc';

const app = document.querySelector<HTMLDivElement>('#app')!;

async function render(): Promise<void> {
  try {
    const [state, settings] = await Promise.all([getState(), getSettings()]);
    app.textContent = JSON.stringify({ camera: state, settings }, null, 2);
  } catch (e) {
    app.textContent = `Cannot reach the app: ${String(e)}`;
  }
  app.style.whiteSpace = 'pre';
  app.style.fontFamily = 'monospace';
}

void render();
void onCameraEvent(() => void render());
