import { describe, expect, test } from 'vitest';
import { connectErrorMessage } from './copy';

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
