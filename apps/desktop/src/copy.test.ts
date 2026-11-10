import { describe, expect, test } from 'vitest';
import {
  askToJoinText,
  connectErrorMessage,
  connectingText,
  findingText,
  joiningText,
  NOT_A_WIFI_CODE,
  reconnectingText,
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

  test('a network the Mac cannot see yet points at the Wi-Fi Direct Info screen', () => {
    expect(wifiErrorMessage('join', ssid, "This computer can't see the network DIRECT-abc1:ZV-E10 yet")).toBe(
      "This Mac can't see DIRECT-abc1:ZV-E10 yet. On the camera, open Menu, Network, PC Remote Function, Wi-Fi Direct Info and stay on that screen, then try again.",
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
