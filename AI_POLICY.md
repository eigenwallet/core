# AI Usage Policy: Smart Coding over Vibe Coding

We do not forbid the use of generative AI tools in this project. 

However, we do not tolerate **Vibe Coding** (flying blind). 
AI is currently _not fit_ to make autonomous changes to this codebase. 
Therefore, any change that is not piloted by a human who understands the issue better than the AI is bound to have problems.

This project is maintained by volunteers who spent their real, personal time reviewing issues and pull requests (PRs).
Do not abuse it

## TL;DR

|AI as tool|✓|
|AI as autopilot|✗|

|Restraining AI, refactor code and prevent slop|✓|
|Let AI run wild|✗|

|Verify tests pass, compile app and test UI before submitting PR|✓|
|Trust AI to produce valid and correct changes|✗|

---

## Our Rules

### 1. You are the Pilot, not the Passenger

**Rule:** You are 100% responsible for every line of code you submit. AI is currently not fit to autonomously make signifcant changes to this codebase. We will hold _you_ accountable for bugs, security vulnerabilities, poor architecture, or generally bad code.

### 2. Verify your changes

**Rule:** For any PR you expect us to review, we expect you to make an effort to validate your claims and changes. We expect you to successfully run and pass the integration test suite. If you make changes to the UI we except you to share screenshots demoing the new look. If you make changes to the build system, we expect you to verify the actual build process (dev _and_ release) and test the resulting application on all platforms you have access to and note which platforms you couldn't test.

**Your Job:** It's up to you to clean up the output, refactor it, and adapt it to our project standards. We expect code you submit to be tested by you personally.

### 3. Transparency

**Rule:** You must indicate whether and which AI tools were used to assist. If we suspect you used AI without disclosing it, we reserve the right to close your PR without comment.

### 4. No "Drive-by" Pull Requests

**Rule:** Don't blindly apply AI-generated "optimizations" or "fixes" without an accepted issue for it.

### 5. Human Communication

**Rule:** Commit messages, PR descriptions, and discussions must be written 100% by yourself. We will not waste our time trying to understand AI hallucinations or arguing with an overconfident LLM. 

**Expectation:** We don't want to read 5-page AI-generated essays about what your code does. We want to hear briefly and concisely from *you*:

- What was the problem?
- How did you solve it?
- Why did you choose this approach?

Do not feel bad if you feel that you're language is imperfect. We would much rather read a real text with slight imperfections that more AI slop.

---

## What We Immediately Reject

We will close PRs and potentially block contributors that exhibit characteristics of _AI Slop_ or lack of effort:

- AI generated titles and descriptions
- bloated code
- untested changes
- Drive-by PRs that don't solve an existing issue
- Shallow, overconfident "security vulnerabilieties"

---

## To the Humans Behind the Code

This project is maintained by humans. Every bad, unreviewed AI PR costs us valuable time for reviews and debugging.

Prove to us that you control the AI, and we look forward to your contribution.

---

