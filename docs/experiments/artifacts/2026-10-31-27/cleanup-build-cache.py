from pathlib import Path
import shutil

workspace=Path('/home/debian/projects/way-toys').resolve(strict=True)
target=workspace/'target/debug/incremental'
resolved=target.resolve(strict=True)
assert resolved==Path('/home/debian/projects/way-toys/target/debug/incremental')
assert resolved.is_relative_to(workspace/'target') and not target.is_symlink()
print('Removing only regenerable Cargo incremental cache:',resolved)
shutil.rmtree(resolved)
