import { describe, expect, it } from 'vitest';
import { parseWifiQr } from './payload';

describe('parseWifiQr', () => {
  it('reads the standard Wi-Fi grammar, including a colon in the name', () => {
    expect(parseWifiQr('WIFI:S:DIRECT-abc1:ZV-E10;T:WPA;P:12345678;;'))
      .toEqual({ ssid: 'DIRECT-abc1:ZV-E10', password: '12345678' });
  });
  it('reads the Sony-style grammar with a model', () => {
    expect(parseWifiQr('W01:S:DIRECT-abc1:ZV-E10;P:12345678;C:ZV-E10;M:aabbccddeeff;'))
      .toEqual({ ssid: 'DIRECT-abc1:ZV-E10', password: '12345678', model: 'ZV-E10' });
  });
  it('unescapes the special characters and accepts an open network', () => {
    expect(parseWifiQr('WIFI:S:Caf\\;e\\:1;T:nopass;;')).toEqual({ ssid: 'Caf;e:1', password: '' });
    expect(parseWifiQr('S:a;P:p\\\\q;')).toEqual({ ssid: 'a', password: 'p\\q' });
  });
  it('rejects text that is not credentials', () => {
    for (const t of ['https://www.sony.net/app', '', 'hello world', 'WIFI:T:WPA;;', 'P:onlypassword;'])
      expect(parseWifiQr(t)).toBeNull();
  });
});
