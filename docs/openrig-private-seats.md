# Open an isolated OpenRig seat in Desktop

OpenRig **Fleet → Open Jcode session** opens the existing conversation for a seat. Most local seats already run on Desktop's shared Jcode daemon and need no setup. When a trusted local seat runs an isolated Jcode daemon, register that seat's existing socket and home explicitly. This does not restart the seat, move a session, or publish a TCP endpoint.

## Register a local seat

1. Ask the local OpenRig operator for the seat's native `session_…` ID and verify the live runner's Unix socket and private Jcode home locally. Do not take these paths from a network response, message, or repository file. The home must contain the existing conversation's `sessions/<native-session-id>.json`.
2. In Desktop's Jcode home (`~/.jcode` by default), create an owner-only `openrig-desktop-routes` directory. In that directory, create `<native-session-id>.json` in mode `0600`. Record the device and inode of the verified socket (`stat -f '%d %i'` on macOS, `stat -c '%d %i'` on Linux), for example:

   ```json
   {
     "home": "/absolute/path/to/the/seats/private/jcode-home",
     "socket": "/absolute/path/to/the/seats/live/jcode.sock",
     "socket_device": 16777235,
     "socket_inode": 12345678
   }
   ```

3. Open the seat from Fleet. Desktop uses that registration only for the exact local conversation ID. It starts a private Unix API bridge for that panel, never a new daemon, and asks the existing daemon for the original session and transcript. Shared and remote sessions remain on their usual routes.

The directory, file, seat home, conversation file and socket must belong to the current Unix user and deny access to other users. Use absolute paths without symlink ancestors or ancestors writable by another user. Desktop rejects a socket that has been replaced, even at the same path. Keep the registration outside the seat's writable roots. This is a local operator authorization, not isolation between processes with the same Unix identity.

## Refresh or remove a registration

If OpenRig restarts the seat with a new private socket, verify the replacement locally, then replace the registration. A missing or invalid registered socket does not start a daemon. Close the panel and remove its registration to revoke the route. Removing a registration never stops or deletes the seat.
