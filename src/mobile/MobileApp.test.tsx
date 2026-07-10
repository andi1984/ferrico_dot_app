import { render, screen } from '@testing-library/react'
import { describe, it, expect } from 'vitest'
import { MobileApp } from './MobileApp'

describe('MobileApp (placeholder)', () => {
  it('renders the app title', () => {
    render(<MobileApp />)
    expect(screen.getByText('Ferrico')).toBeInTheDocument()
  })
})
