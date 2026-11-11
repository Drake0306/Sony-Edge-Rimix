import { describe, expect, test } from 'vitest';
import {
  askToJoinText,
  BLE_UNAVAILABLE,
  bleCameraName,
  bleFailedText,
  bleNearbyText,
  bleReadingText,
  isBleUnavailable,
  PAIRING_OFF,
  PAIRING_OPEN,
  connectErrorMessage,
  connectingText,
  findingText,
  fingerprintChangedText,
  fingerprintText,
  joiningText,
  NOT_A_WIFI_CODE,
  reconnectingText,
  sshErrorMessage,
  wifiErrorMessage,
} from './copy';

describe('connectErrorMessage', () => {
  test('connect_error_message_for_refused', () => {
    expect(
      connectErrorMessage(
        'Cannot connect to 192.168.122.1:15740: I/O error: Connection refused (os error 61)',
        '192.168.122.1',
      ),
    ).toBe(
      'Could not reach the camera at 192.168.122.1. Check that PC Remote is on and you are on the camera\'s Wi-Fi.',
    );
    // Windows words it differently.
    expect(
      connectErrorMessage(
        'Cannot connect to 10.0.0.2:15740: I/O error: No connection could be made because the target machine actively refused it. (os error 10061)',
        '10.0.0.2',
      ),
    ).toContain('Could not reach the camera at 10.0.0.2.');
  });

  test('timeout during pairing asks to press OK', () => {
    expect(
      connectErrorMessage(
        'Cannot connect to 192.168.122.1:15740: the camera did not confirm the pairing in time',
        '192.168.122.1',
      ),
    ).toBe('The camera asked you to allow this computer. Press OK on the camera, then try again.');
  });

  test('a connect timeout or an unreachable host is not about pairing', () => {
    const reach =
      "Could not reach the camera at 192.168.122.1. Check that PC Remote is on and you are on the camera's Wi-Fi.";
    for (const err of [
      'Cannot connect to 192.168.122.1:15740: operation timed out',
      'Cannot connect to 192.168.122.1:15740: I/O error: No route to host (os error 65)',
      'Cannot connect to 192.168.122.1:15740: I/O error: Host is down (os error 64)',
      'Cannot connect to 192.168.122.1:15740: camera disconnected',
    ]) {
      expect(connectErrorMessage(err, '192.168.122.1'), err).toBe(reach);
    }
  });

  test('anything else names the camera and keeps the detail', () => {
    expect(
      connectErrorMessage('Cannot connect to 1.2.3.4:15740: protocol error: bad packet', '1.2.3.4'),
    ).toBe('Could not connect to the camera at 1.2.3.4: protocol error: bad packet');
    expect(connectErrorMessage('Not an IP address: cam', 'cam')).toBe(
      'Could not connect to the camera at cam: Not an IP address: cam',
    );
  });
});

describe('Wi-Fi connect copy', () => {
  const ssid = 'DIRECT-abc1:ZV-E10';

  test('progress lines name the network or camera', () => {
    expect(joiningText(ssid)).toBe('Joining DIRECT-abc1:ZV-E10');
    expect(findingText(ssid)).toBe('Looking for the camera on DIRECT-abc1:ZV-E10');
    expect(askToJoinText(ssid)).toBe(
      'Join DIRECT-abc1:ZV-E10 from the Wi-Fi menu. Rimix connects as soon as the camera appears.',
    );
    expect(connectingText('ZV-E10')).toBe('Connecting to ZV-E10. If the camera asks, press OK on it.');
  });

  test('a failed join keeps the detail and points at the Wi-Fi menu', () => {
    expect(wifiErrorMessage('join', ssid, 'Joining Wi-Fi networks is not supported on this system yet')).toBe(
      'Could not join DIRECT-abc1:ZV-E10: Joining Wi-Fi networks is not supported on this system yet. Join it from the Wi-Fi menu, then try again.',
    );
  });

  test('a detail that ends in a full stop is not doubled', () => {
    expect(wifiErrorMessage('join', ssid, 'Failed to join network DIRECT-abc1:ZV-E10.')).toBe(
      'Could not join DIRECT-abc1:ZV-E10: Failed to join network DIRECT-abc1:ZV-E10. Join it from the Wi-Fi menu, then try again.',
    );
  });

  test('a network this computer cannot see yet points at the Wi-Fi Direct Info screen', () => {
    expect(wifiErrorMessage('join', ssid, "This computer can't see the network DIRECT-abc1:ZV-E10 yet")).toBe(
      "This computer can't see DIRECT-abc1:ZV-E10 yet. On the camera, open Menu, Network, PC Remote Function, Wi-Fi Direct Info and stay on that screen, then try again.",
    );
  });

  test('no Wi-Fi at all suggests Nearby', () => {
    expect(wifiErrorMessage('join', ssid, 'This computer has no Wi-Fi')).toBe(
      'This computer has no Wi-Fi. Put the camera and this computer on the same router and use Nearby.',
    );
  });

  test('a silent network points at PC Remote Function', () => {
    expect(wifiErrorMessage('find', ssid, 'No camera answered on this network')).toBe(
      'Joined DIRECT-abc1:ZV-E10 but no camera answered. On the camera, open Menu, Network, PC Remote Function, and stay on that screen.',
    );
  });

  test('a silent network after asking to join does not claim Rimix joined it', () => {
    expect(wifiErrorMessage('wait', ssid, 'No camera answered on this network')).toBe(
      'No camera answered on DIRECT-abc1:ZV-E10. Check this computer is on that network, then on the camera open Menu, Network, PC Remote Function, and stay on that screen.',
    );
  });

  test('a system that keeps Rimix off the local network points at its privacy setting', () => {
    const blocked = 'This computer is blocking Rimix from the local network';
    expect(wifiErrorMessage('find', ssid, blocked)).toBe(
      'Joined DIRECT-abc1:ZV-E10, but macOS is keeping Rimix off the local network, so it cannot reach the camera. Open System Settings, Privacy & Security, Local Network, turn Rimix on (if it is on, turn it off and on again), then quit and reopen Rimix.',
    );
    expect(wifiErrorMessage('wait', ssid, blocked)).toBe(
      'macOS is keeping Rimix off the local network, so it cannot reach the camera. Open System Settings, Privacy & Security, Local Network, turn Rimix on (if it is on, turn it off and on again), then quit and reopen Rimix.',
    );
  });

  test('text that is not a Wi-Fi code', () => {
    expect(NOT_A_WIFI_CODE).toBe(
      "This code isn't a camera Wi-Fi code. Check the text below, or type the name and password.",
    );
  });
});

describe('reconnectingText', () => {
  test('is short for the first attempts and explains from the fifth', () => {
    expect(reconnectingText(1)).toBe('Reconnecting');
    expect(reconnectingText(4)).toBe('Reconnecting');
    expect(reconnectingText(5)).toBe("Reconnecting… if the camera's Wi-Fi dropped, rejoin it from the Wi-Fi menu");
  });
});

describe('sshErrorMessage', () => {
  const AUTH = 'The camera refused the username or password. Check Access Authen. Info on the camera and try again.';
  const CHANGED =
    "The camera's fingerprint changed. If you reset the camera, forget it in App settings and connect again; otherwise do not connect.";

  test('a refused login and a changed fingerprint have their own words', () => {
    expect(sshErrorMessage('the camera refused the username or password')).toBe(AUTH);
    expect(
      sshErrorMessage("the camera's fingerprint changed (expected SHA256:aaa, got SHA256:bbb)"),
    ).toBe(CHANGED);
    expect(sshErrorMessage('I/O error: Connection refused (os error 61)')).toBeNull();
  });

  test('connectErrorMessage uses them rather than calling the camera unreachable', () => {
    expect(
      connectErrorMessage(
        'Cannot connect to 192.168.122.1:15740: the camera refused the username or password',
        '192.168.122.1',
      ),
    ).toBe(AUTH);
    expect(
      connectErrorMessage(
        "Cannot connect to 192.168.122.1:15740: the camera's fingerprint changed (expected SHA256:a, got SHA256:b)",
        '192.168.122.1',
      ),
    ).toBe(CHANGED);
  });
});

describe('fingerprint copy', () => {
  test('the confirmation shows both hashes', () => {
    expect(fingerprintText('SHA256:abc', 'aa:bb')).toBe(
      "This camera's fingerprint is SHA256:abc (MD5 aa:bb). Check it against Access Authen. Info on the camera; it may show either form.",
    );
  });

  test('a changed fingerprint shows the stored and the current one', () => {
    expect(fingerprintChangedText('SHA256:old', 'SHA256:new')).toBe(
      "This camera's fingerprint changed. Stored: SHA256:old. Camera now: SHA256:new. If you reset the camera, forget it in App settings and connect again; otherwise do not connect.",
    );
  });
});

describe('Bluetooth', () => {
  const zv = {
    address: 'fake-zv-e10',
    advert: { name: 'ZV-E10', model_code: 'E1', pairing_open: true, address: 'fake-zv-e10' },
  };

  test('a camera is called by its advertised name, else by its model code', () => {
    expect(bleCameraName(zv)).toBe('ZV-E10');
    expect(bleNearbyText(zv)).toBe('ZV-E10 nearby');
    const unnamed = { ...zv, advert: { ...zv.advert, name: null } };
    expect(bleCameraName(unnamed)).toBe('Sony camera E1');
    expect(bleNearbyText(unnamed)).toBe('Sony camera E1 nearby');
  });

  test('the badges and progress line', () => {
    expect(PAIRING_OPEN).toBe('pairing open');
    expect(PAIRING_OFF).toBe('pairing off');
    expect(bleReadingText('ZV-E10')).toBe('Reading Wi-Fi details from ZV-E10 over Bluetooth');
  });

  test('a failed handoff points at the other ways in', () => {
    expect(bleFailedText('Bluetooth failed: timed out after 5000 ms waiting to connect')).toBe(
      'Rimix could not read the Wi-Fi details over Bluetooth (timed out after 5000 ms waiting to connect). Scan the QR code or type the name and password instead.',
    );
    expect(bleFailedText('no such Bluetooth device or characteristic')).toBe(
      'Rimix could not read the Wi-Fi details over Bluetooth (no such Bluetooth device or characteristic). Scan the QR code or type the name and password instead.',
    );
  });

  test('unavailable Bluetooth is told by its error and says so quietly', () => {
    expect(isBleUnavailable('Bluetooth unavailable: Bluetooth is turned off')).toBe(true);
    expect(isBleUnavailable('Bluetooth failed: timed out')).toBe(false);
    expect(BLE_UNAVAILABLE).toBe('Bluetooth is off or not allowed; nearby cameras come from Wi-Fi only.');
  });
});
