# Claude Code Quality Guidance

This document provides guidance for Claude Code to maintain consistent quality and best practices when working on this repository.

## Code Style & Standards

- Follow the existing code style and conventions in the repository
- Write clear, concise code that is easy to understand
- Use meaningful variable and function names
- Keep functions focused and single-purpose

## Before Making Changes

- Always read existing code before proposing modifications
- Understand the existing architecture and patterns
- Check for similar implementations before adding new code
- Avoid over-engineering solutions

## Implementation Guidelines

- Make only the changes that are requested or clearly necessary
- Don't add features, refactoring, or improvements beyond the scope
- Don't add error handling for scenarios that can't happen
- Trust internal code and framework guarantees
- Validate input only at system boundaries (user input, external APIs)
- Avoid creating unnecessary abstractions or utilities

## Security

- Be aware of common vulnerabilities (OWASP Top 10)
- Validate user input and external data
- Avoid command injection, XSS, SQL injection, and similar issues
- Review code changes for security implications

## Git Practices

- Write clear, descriptive commit messages
- Commit related changes together
- Keep commits focused and logical
- Push to the designated branch as specified in development requirements

## Testing & Validation

- Run tests before committing changes
- Ensure existing functionality is not broken
- Write tests for new functionality when applicable
- Check for type errors and linting issues

## Documentation

- Keep README and documentation up-to-date
- Add comments only when logic is not self-evident
- Document breaking changes and migrations
