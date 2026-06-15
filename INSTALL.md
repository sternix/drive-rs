cd backend
cp .env.example .env

node -e "console.log(require('crypto').randomBytes(256).toString('base64'));"
çıktığı .env dosyasındaki JWT_SECRET değeri olarak gir

create user drive_usr;
\password drive_usr
create database drive with owner drive_usr;

DATABASE_URL=postgresql://user:password@host:port/dbname
DATABASE_URL=postgresql://drive_usr:secret@localhost:5432/drive

mkdir -p /opt/drive/uploads
cp example.env /opt/drive/.env
