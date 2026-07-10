import { describe, it, expect, afterEach } from 'vitest'
import { isMobilePlatform } from './platform'

function setUserAgent(ua: string) {
  Object.defineProperty(navigator, 'userAgent', { value: ua, configurable: true })
}

const ORIGINAL_UA = navigator.userAgent

describe('isMobilePlatform', () => {
  afterEach(() => {
    setUserAgent(ORIGINAL_UA)
  })

  it('detects Android', () => {
    setUserAgent('Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36')
    expect(isMobilePlatform()).toBe(true)
  })

  it('detects iPhone', () => {
    setUserAgent('Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15')
    expect(isMobilePlatform()).toBe(true)
  })

  it('detects iPad', () => {
    setUserAgent('Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) AppleWebKit/605.1.15')
    expect(isMobilePlatform()).toBe(true)
  })

  it('does not flag desktop Linux', () => {
    setUserAgent('Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36')
    expect(isMobilePlatform()).toBe(false)
  })

  it('does not flag desktop macOS', () => {
    setUserAgent('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15')
    expect(isMobilePlatform()).toBe(false)
  })

  it('does not flag desktop Windows', () => {
    setUserAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36')
    expect(isMobilePlatform()).toBe(false)
  })
})
