// channel.html (old channel links, §27): channels are part of the app now (Appendix F.3.3).
// A channel link opens the app's reader; the page alone opens My channels.
location.replace(`tor.html${location.hash.startsWith('#c=') ? location.hash : '#tab=own'}`);
