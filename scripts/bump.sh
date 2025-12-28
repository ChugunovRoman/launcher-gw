#!/bin/bash

NEW_VERSION=$1

if [ -z "$NEW_VERSION" ]; then
  echo "Ошибка: укажите новую версию. Пример: ./update_version.sh 1.2.3"
  exit 1
fi

echo "Обновление версии до $NEW_VERSION..."

sed -i "s/\"version\": \".*\"/\"version\": \"$NEW_VERSION\"/" package.json

if [ -f "src-tauri/tauri.conf.json" ]; then
  sed -i "s/\"version\": \".*\"/\"version\": \"$NEW_VERSION\"/" src-tauri/tauri.conf.json
fi

if [ -f "src-tauri/Cargo.toml" ]; then
  sed -i "0,/version = \".*\"/s//version = \"$NEW_VERSION\"/" src-tauri/Cargo.toml
fi

git add .
git commit -m "Release launcher v$NEW_VERSION"
git tag -a $NEW_VERSION -m "Release launcher $NEW_VERSION"
git push --tags origin master

echo "Готово! Все файлы обновлены."
