# iPhone Shortcuts

Two Shortcuts send things to your companion from anywhere: **Send to Xteink** (a web page
from the Share Sheet) and **Send summary** (any text, such as an email summary). Both reach
the companion through Cloudflare Access with a service token, so they work without a browser
login. The web page is still the place for tokens and pictures.

In this guide, `reader.example.org` stands for your companion's address. Use your own.

## 1. Make a service token (Cloudflare, once)

1. In Cloudflare Zero Trust, open **Access → Service credentials → Service Tokens**, and
   choose **Create Service Token**. Name it `x3-shortcut`. Pick a duration (one year is fine;
   set a reminder to renew it).
2. Copy the **Client ID** and **Client Secret**. Cloudflare shows the secret only once. Keep
   both in your password manager, not in a note.
3. Open the Access application for your companion's address. Add a policy:
   - Action: **Service Auth** (not Allow: an Allow policy ignores service tokens and sends
     the Shortcut to the login page).
   - Include: **Service Token** → `x3-shortcut`.
4. Check it from a computer. The answer must be JSON, not a web page:

   ```
   curl -H "CF-Access-Client-Id: ID" -H "CF-Access-Client-Secret: SECRET" \
        https://reader.example.org/api/v1/device
   ```

   `{"last_contact":…}` means it works. A page of HTML means the policy in step 3 is
   missing or is an Allow policy.

To stop a lost phone from sending, revoke the token in Cloudflare. Nothing on the companion
changes.

## 2. The submission ID

Every request carries a `submission_id`, so a request sent twice is queued once. It must
look like a UUID. Shortcuts has no UUID action, so both Shortcuts build one from the date
and a random number. Digits are valid in a UUID, and the companion accepts this form:

1. **Format Date**: Current Date, Custom format `yyyyMMdd-HHmm-'4'ssS-'8'SSS`
2. **Random Number** between `100000000000` and `999999999999`
3. **Text**: `Formatted Date-Random Number` (the two results with a hyphen between)

The result looks like `20261102-1530-4071-8123-482910384756`. Make this a small Shortcut of
its own, called **X3 ID**, ending with **Stop and Output** (the Text), and call it with **Run
Shortcut** from the two below.

## 3. Send to Xteink (Share Sheet)

The companion opens the page, keeps the article text and prepares it for the X3.

1. Shortcut details: **Show in Share Sheet** on; it receives **URLs** and **Safari web
   pages**. If there is no input: **Ask For** URL.
2. **Get URLs from Input** (Shortcut Input).
3. **Choose from Menu** with the prompt "Send as": **Article** and **Summary**. In each
   branch, **Text** `article` or `summary`, then **Set Variable** `kind`.
4. **Run Shortcut** `X3 ID`, then **Set Variable** `id`.
5. **Get Contents of URL**:
   - URL: `https://reader.example.org/api/v1/articles/link`
   - Method: **POST**
   - Headers: `CF-Access-Client-Id` = your ID, `CF-Access-Client-Secret` = your secret
   - Request Body: **JSON**, with three Text fields:
     `submission_id` = `id`, `kind` = `kind`, `url` = the URL from step 2
6. **Get Dictionary Value** `job` from Contents of URL.
7. **If** Dictionary Value **has any value**: **Show Notification** "Queued for the X3.
   Press Receive when you want it." **Otherwise**: **Show Result** (Contents of URL), so
   you can see what went wrong.

The page must open without a login and show the article in its HTML. Pages behind a
paywall or built only by JavaScript fail; the inbox on the web page then shows "Failed". Use
Send summary with the text instead.

## 4. Send summary (any text)

For an email summary, a note, or text you copied from an app.

1. Shortcut details: **Show in Share Sheet** on; it receives **Text**. If there is no
   input: **Get Clipboard**.
2. **Set Variable** `text` to the input.
3. **Ask for Input**, Text, prompt "Title", default "Summary". **Set Variable** `title`.
4. Optional, on iOS with Apple Intelligence: **Summarise** `text` and use the result as
   `text`. Leave it out to send the text as it is.
5. **Run Shortcut** `X3 ID`, then **Set Variable** `id`.
6. **Get Contents of URL**:
   - URL: `https://reader.example.org/api/v1/articles`
   - Method: **POST**, the same two headers as above
   - Request Body: **JSON**, Text fields `submission_id` = `id`, `kind` = `summary`,
     `title` = `title`, `source_url` = empty, `text` = `text`
7. The same check as step 7 above.

A line that starts with `# ` becomes a heading on the X3. Leave a blank line between
paragraphs. The text can be up to 1 MB.

## 5. Optional: the NFC sticker

The X3's NFC tag is not connected to the device, but iPhone Automations can start on it.
In **Shortcuts → Automation → NFC**, scan the sticker and choose **Open URL**
`https://reader.example.org/`. Holding the phone to the X3 then opens the companion page.

## When something goes wrong

| What you see | Why | Fix |
| --- | --- | --- |
| A Cloudflare login page | The Access policy is Allow, or the headers are missing | Step 1.3; check the header names |
| `{"error":"unauthorised"}` | The token expired or was revoked | Make a new token |
| `{"error":"invalid_request"}` | A field is missing or the ID is not in UUID shape | Check the JSON field names and the X3 ID Shortcut |
| `{"error":"conflict"}` | The same ID was sent with a different body | Run the Shortcut again; it makes a new ID |
| `{"error":"storage_full"}` | The inbox is full | Delivered items are cleared after 30 days |
