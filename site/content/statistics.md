## Network Statistics

<!-- Hero Metric -->
<div style="text-align: center; margin: 2rem 0 3rem 0; padding: 2rem; border: 2px solid #ff6b35; border-radius: 8px; background: #fafafa;">
  <div style="font-size: 3.5rem; font-weight: bold; margin-bottom: 0.5rem; color: #ff6b35;">{{TOTAL_LIQUIDITY}} BTC</div>
  <div style="font-size: 1.2rem; color: #666;">Total Network Liquidity</div>
</div>

<!-- Supporting Metrics Table -->
<div style="margin: 2rem 0;">
  <table style="width: 100%; border-collapse: collapse; margin: 0; background: white;">
    <thead>
      <tr style="background: #f8f9fa;">
        <th style="padding: 1rem; text-align: left; border-bottom: 2px solid #dee2e6; font-weight: 600;">Metric</th>
        <th style="padding: 1rem; text-align: right; border-bottom: 2px solid #dee2e6; font-weight: 600;">Value</th>
      </tr>
    </thead>
    <tbody>
      <tr style="border-bottom: 1px solid #dee2e6;">
        <td style="padding: 1rem; color: #495057;">Maximum Swap Amount</td>
        <td style="padding: 1rem; text-align: right; font-weight: 600; color: #ff6b35;">{{MAX_SWAP}} BTC</td>
      </tr>
      <tr style="border-bottom: 1px solid #dee2e6;">
        <td style="padding: 1rem; color: #495057;">Minimum Swap Amount</td>
        <td style="padding: 1rem; text-align: right; font-weight: 600; color: #ff6b35;">{{MIN_SWAP}} BTC</td>
      </tr>
      <tr>
        <td style="padding: 1rem; color: #495057;">Total Downloads</td>
        <td style="padding: 1rem; text-align: right; font-weight: 600; color: #ff6b35;">{{TOTAL_DOWNLOADS}}</td>
      </tr>
    </tbody>
  </table>
</div>

### Liquidity Over Time

<div style="margin: 2rem 0; text-align: center;">
  {{LIQUIDITY_CHART}}
</div>

### Top Makers Leaderboard

{{LEADERBOARD_RECENT}}

{{LEADERBOARD_ALLTIME}}

**Note**: *If your maker is not in the leaderboard, it does not mean it is not online or not discoverable to other peers. This site cannot have a full view of the entire state of the peer to peer network at all times.*
 
---

*Statistics updated on {{LAST_UPDATED}}.* 