import { describe, it, expect } from 'vitest'
import { domainOf, initials, extractErrorMessage, formatDate, duckduckgoFavicon } from './utils'

describe('duckduckgoFavicon', () => {
  it('returns favicon url for valid url', () => {
    expect(duckduckgoFavicon('https://example.com')).toBe(
      'https://icons.duckduckgo.com/ip3/example.com.ico'
    )
  })
  it('returns empty string for invalid url', () => {
    expect(duckduckgoFavicon('not-a-url')).toBe('')
  })
})

describe('domainOf', () => {
  it('strips www prefix', () => {
    expect(domainOf('https://www.example.com/path')).toBe('example.com')
  })
  it('preserves subdomains', () => {
    expect(domainOf('https://docs.example.com')).toBe('docs.example.com')
  })
  it('returns input unchanged for invalid url', () => {
    expect(domainOf('not-a-url')).toBe('not-a-url')
  })
  it('strips trailing path', () => {
    expect(domainOf('https://github.com/user/repo')).toBe('github.com')
  })
})

describe('initials', () => {
  it('uppercases first character', () => {
    expect(initials('hello')).toBe('H')
  })
  it('trims leading whitespace before picking char', () => {
    expect(initials('  world')).toBe('W')
  })
  it('returns ? for empty string', () => {
    expect(initials('')).toBe('?')
  })
  it('returns ? for whitespace-only string', () => {
    expect(initials('   ')).toBe('?')
  })
})

describe('extractErrorMessage', () => {
  it('returns a string as-is', () => {
    expect(extractErrorMessage('oops')).toBe('oops')
  })
  it('extracts .message from typed AppError object', () => {
    expect(extractErrorMessage({ name: 'Db', message: 'connection failed' })).toBe(
      'connection failed'
    )
  })
  it('extracts .message from plain Error', () => {
    expect(extractErrorMessage(new Error('something went wrong'))).toBe('something went wrong')
  })
  it('stringifies numbers', () => {
    expect(extractErrorMessage(42)).toBe('42')
  })
  it('stringifies null', () => {
    expect(extractErrorMessage(null)).toBe('null')
  })
})

describe('formatDate', () => {
  it('returns Today for a timestamp in the last 24 hours', () => {
    const now = Math.floor(Date.now() / 1000)
    expect(formatDate(now)).toBe('Today')
  })
  it('returns Yesterday for ~1 day ago', () => {
    const yesterday = Math.floor(Date.now() / 1000) - 86400
    expect(formatDate(yesterday)).toBe('Yesterday')
  })
  it('returns Xd ago for 3 days ago', () => {
    const threeDays = Math.floor(Date.now() / 1000) - 86400 * 3
    expect(formatDate(threeDays)).toBe('3d ago')
  })
  it('returns formatted date for older timestamps', () => {
    const oldTs = Math.floor(new Date('2020-06-15').getTime() / 1000)
    const result = formatDate(oldTs)
    // Should contain month abbreviation and day
    expect(result).toMatch(/Jun|Jul/)
  })
})
