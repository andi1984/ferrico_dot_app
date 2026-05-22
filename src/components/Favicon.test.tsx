import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect } from 'vitest'
import { Favicon } from './Favicon'

// <img alt=""> is a decorative image with role "presentation", not "img".
// Query by selector instead of ARIA role.
function getImg() {
  return document.querySelector('img')
}

describe('Favicon', () => {
  it('renders an img with storedUrl when provided', () => {
    render(<Favicon storedUrl="https://cdn.example.com/icon.png" bookmarkUrl="https://example.com" title="Example" />)
    expect(getImg()).toHaveAttribute('src', 'https://cdn.example.com/icon.png')
  })

  it('renders an img with duckduckgo favicon when storedUrl is null', () => {
    render(<Favicon storedUrl={null} bookmarkUrl="https://example.com" title="Example" />)
    expect(getImg()).toHaveAttribute('src', 'https://icons.duckduckgo.com/ip3/example.com.ico')
  })

  it('falls back to initials when the image fails to load', () => {
    render(<Favicon storedUrl={null} bookmarkUrl="https://example.com" title="Example" />)
    fireEvent.error(getImg()!)
    expect(getImg()).toBeNull()
    expect(screen.getByText('E')).toBeInTheDocument()
  })

  it('renders initials immediately when no valid favicon URL can be derived', () => {
    render(<Favicon storedUrl={null} bookmarkUrl="not-a-url" title="Test" />)
    expect(getImg()).toBeNull()
    expect(screen.getByText('T')).toBeInTheDocument()
  })
})
