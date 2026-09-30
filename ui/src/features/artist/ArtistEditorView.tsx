import { CircleUserRound } from "lucide-react";
import { useEffect, useState } from "react";
import { api, artworkUrl, type LoadedImage } from "../../lib/api";
import { FLSheet, Modal } from "../../components/sheet/Sheet";
import { PillButton } from "../../components/settings/Primitives";
import { ImageCropper, pickImage } from "../../components/editors/ImageCropper";
import "../../components/editors/Editor.css";

type Target = "banner" | "profile";

/**
 * `ArtistEditorView`: display-name override, 3:1 banner and 1:1 profile
 * picture. Overrides only; tags are never touched.
 */
export function ArtistEditorView({
  artistKey,
  fallbackName,
  fallbackArtwork,
  onClose,
}: {
  artistKey: string;
  fallbackName: string;
  fallbackArtwork: string | null;
  onClose: () => void;
}) {
  const [displayName, setDisplayName] = useState("");
  const [banner, setBanner] = useState<string | null>(null);
  const [profile, setProfile] = useState<string | null>(null);
  const [crop, setCrop] = useState<{ source: LoadedImage; target: Target } | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void api.artistOverride(artistKey).then((o) => {
      if (!o) return;
      setDisplayName(o.displayName ?? "");
      setBanner(o.banner);
      setProfile(o.profile);
    });
  }, [artistKey]);

  const choose = async (target: Target) => {
    setError(null);
    try {
      const source = await pickImage(target === "banner" ? "Choose a banner image" : "Choose a profile image");
      if (source) setCrop({ source, target });
    } catch (e) {
      setError(String(e));
    }
  };

  const save = async () => {
    await api.saveArtistOverride(artistKey, displayName.trim() || null, banner, profile);
    onClose();
  };

  const reset = async () => {
    await api.resetArtistOverride(artistKey);
    onClose();
  };

  const bannerShown = banner ?? fallbackArtwork;

  return (
    <FLSheet
      title="Edit Artist"
      width={580}
      height={620}
      onClose={onClose}
      footer={
        <div className="editor-footer">
          <PillButton muted onClick={() => void reset()}>
            Reset to Default
          </PillButton>
          <div style={{ flex: 1 }} />
          <PillButton onClick={onClose}>Cancel</PillButton>
          <PillButton primary onClick={() => void save()}>
            Save
          </PillButton>
        </div>
      }
    >
      <div className="editor-form">
        <label className="editor-field">
          <span className="editor-field__label">Display Name</span>
          <input
            className="editor-input"
            value={displayName}
            placeholder={fallbackName}
            onChange={(e) => setDisplayName(e.target.value)}
            autoFocus
          />
          <span className="editor-field__note">Override only — original tags are not modified.</span>
        </label>

        <div className="editor-field">
          <span className="editor-field__label">Banner Image</span>
          <button className="artist-editor__banner" title="Click to choose a banner image" onClick={() => void choose("banner")}>
            {bannerShown && <img src={artworkUrl(bannerShown, 540)} alt="" draggable={false} />}
            {!banner && <span className="artist-editor__hint">Click to choose a banner</span>}
          </button>
          <div className="editor-row">
            {banner && (
              <button className="editor-text-button" onClick={() => setBanner(null)}>
                Remove Banner
              </button>
            )}
            <div style={{ flex: 1 }} />
            <span className="editor-field__note">Cropped to a 3:1 banner.</span>
          </div>
        </div>

        <div className="editor-field">
          <span className="editor-field__label">Profile Image</span>
          <div className="editor-row is-top">
            <button className="artist-editor__profile" title="Click to choose a profile image" onClick={() => void choose("profile")}>
              {profile ? <img src={artworkUrl(profile, 100)} alt="" draggable={false} /> : <CircleUserRound size={30} strokeWidth={0.8} />}
            </button>
            <div className="editor-column">
              <span className="editor-field__note">Shown as a circular avatar on the artist page. Cropped to 1:1.</span>
              {profile && (
                <button className="editor-text-button" onClick={() => setProfile(null)}>
                  Remove Profile Image
                </button>
              )}
            </div>
          </div>
        </div>
        {error && <div className="editor-error">{error}</div>}
      </div>

      <Modal open={crop != null} onClose={() => setCrop(null)}>
        {crop && (
          <ImageCropper
            source={crop.source}
            aspectRatio={crop.target === "banner" ? 3 : 1}
            title={crop.target === "banner" ? "Crop Banner" : "Crop Profile Image"}
            onComplete={(id) => (crop.target === "banner" ? setBanner(id) : setProfile(id))}
            onClose={() => setCrop(null)}
          />
        )}
      </Modal>
    </FLSheet>
  );
}
